# Interface text sources

Spec: text/interface-text
Requirements: R156, R191, R192, R193, R194, R195, R1448
Anki: 26.09.3 (which texts Meguri shows as Anki writes them; the texts themselves are in the files I3 and I4 name)
Scenarios: none (U167's string lint and each catalog's own test; the oracle checks some core texts through the observations their files give, I8)

Where every user-visible text comes from, so that U167's string lint, the String Catalog tests and the screens agree on which texts follow R191 to R193 in full and which keep a fixed English. There are four sources (I2 to I5). Each text belongs to exactly one. This file names the sources and the rules that differ between them; it restates no string.

## I1. Rules by source

| Rule | Meguri text (I2) | Spec-fixed catalog text (I3) | Core text (I4) | Collection and outside content (I5) |
|---|---|---|---|---|
| In a String Catalog with a developer comment (R191) | yes | yes | no | no |
| English value fixed by a spec table | no | yes | yes (the core writes it) | no |
| Plural variants for counts (R191) | yes | as the file's plural rule says | no (the core's text already chooses the form) | no |
| Numbers by the user's locale formatters (R192) | yes | as the file says (I6) | as I6 says per text | not applicable |
| May be translated in a later language (R195) | yes | yes, from the English | as the row's file says (I4) | never (R194) |
| Leading and trailing alignment (R193) | yes | yes | yes, for the element that shows it | yes for the element that shows a name or description; card content keeps its note type's own styling |

- R195: the first release ships English only, so every source shows English today. The rules above say what a later language may change.
- A screen never builds a user-visible sentence by joining texts (R191). A core text (I4) or a content text (I5) is shown in its own text element, or as a whole argument of a catalog entry (for example `%@`), and no catalog text is joined to it. It is never shortened, except where I5 lets a screen shorten a name for layout.

## I2. Meguri text

Every text no spec table fixes: Meguri's own wording, from the String Catalog of the feature that shows it, under R191 to R193 in full. The Product Contract's quoted texts (for example R1309's "No reviews yet", R1438's "Open Meguri to get started", R164's empty states) are this source: their English comes from the Product Contract, not from Anki.

## I3. Spec-fixed catalog text

Catalog entries whose English must equal a spec table, mostly because it is Anki's or the Review Heatmap add-on's wording (KTD1), sometimes because a spec fixes Meguri's own wording (`Anki: none`). Each file names its catalog, its keys and its own catalog test.

| Spec | Catalog | Where it shows | U167's shell scope |
|---|---|---|---|
| `review/reviewer-strings` | Review | deck page finished state and summary (S8, S9), undo and redo titles (S5), the review screen | S5, S8, S9 |
| `stats/activity-strings` | Stats (the widget: `stats/widget-families` F4) | Home and deck-page activity section (S2), the widget, Settings › Heatmap (S3) | yes |
| `stats/heatmap-presentation` M17 | Stats | Home and deck-page heatmap, the Stats Calendar | Home and deck page |
| `sync/status-strings` | Sync (`Anki: none`) | toolbar sync button, status popover, Settings › Account & Sync | yes |
| `package/transfer-strings` | Settings | import and export sheets (U162; Settings › Collection keeps file import and export, N11.4) | yes |
| `maintenance/maintenance-strings` | Settings | Settings › Maintenance and its reports (U163), the Tools menu (U159), the damaged-collection screen's Check Database (R169) | yes |
| `editor/editing-strings` | Editor, Settings, Note Types, Image Occlusion (E1) | add and edit screens, Settings › Editing (E4), note types, image occlusion | E4 only |
| `editor/image-occlusion-strings` | Image Occlusion (`Anki: none` for its English) | image occlusion screens | no |
| `decks/deck-options-strings` | Deck Options | deck options | no |
| `browser/browse-strings` | Browse | Browse | no |
| `stats/graph-presentation` P19, `stats/card-info` C12 labels | Stats | Stats and Card Info | no |

- **Checks.** U167's `lint-strings` checks every catalog that exists for the rules the plan gives it: a developer comment on every entry, plural variants on count strings, no joined user-visible text in Swift, and the I7 word check. The English value of each I3 key is checked by the catalog test its file names, against its table as written (for example `SyncStringCatalogTests`, U228's and U229's tests, U232's test). Where a file names "U167's string lint" as that check (`review/reviewer-strings` S12, `browser/browse-strings`), the check belongs to the unit that adds the catalog and runs in the same `tools/mg lint-ui` job. A file whose catalog does not exist yet is not checked; the unit that adds the catalog adds its file's check with it. How a check reads the expected table (a checked-in list or the spec's table) is that unit's choice; the values must equal the spec's.
- Some files fix English in a table without keys: `editor/formatting` F19 and `editor/paste` P21 (Anki's English kept where they say so), `stats/studied-today` T4.3's `Today` heading. Their own tests check them.
- A file that later adds spec-fixed wording adds a row here (edit request) and its own test.

## I4. Core text

Texts the core or the card runtime writes, in Anki's English unless the file names Meguri's wording, which the UI shows exactly as returned. They are not catalog entries and Swift formats no number in them. Whether a later language may translate one is its file's rule, given in the last column; R1448 and the plan's Open Questions default (Card Info display strings verbatim in every locale) fix it for the first two rows, and the other files fix it for theirs.

| Text | Spec | Shown on | Later languages |
|---|---|---|---|
| The Home studied-today line | `stats/studied-today` T2, T6.1, T6.3 | Home, deck page activity, the large widget (`stats/widget-snapshot` W3 `studiedToday.text`, `stats/widget-families` F5) | English in every locale (R1448, T6.3) |
| The Stats Today lines | `stats/studied-today` T4, T6.2, T6.3 | Stats | English in every locale (T6.3) |
| Card Info value texts (dates, spans, kinds, percentages) | `stats/card-info` C12 "(core)" rows | Card Info | verbatim in every locale (R1448, Open Questions) |
| Answer button intervals (`<10m`, `1d`, `2.3mo`, `(end)`), visible form | `sched/answer-labels` A3 to A6; `review/reviewer-strings` S13 (the interval is taken from the core, never from the catalog; its spoken forms are catalog text) | review | open: R192 asks for localizable interval labels on buttons and in the summary; no spec yet says how a later language changes the visible label (A6: English only, as the interface is) |
| Blank-card and template-error notices (D8's message is Meguri's wording) | `render/display-transform` D7, D8, D10 | review, preview | English in every interface language (`render/preview` V7, citing D7) |
| Type-answer warnings | `render/type-answer` A8 | review | English; other languages from Meguri's String Catalog (A8) |
| The preview's sample cloze | `render/preview` V7 | note type preview | English in every interface language (V7) |
| Image Occlusion's `No image to show.` | `render/image-occlusion-review` M9 | review | English for the first release (R195); a later language is not yet specified |

- The lint never asks for a catalog entry, a comment or plural variants for these texts, and never flags them as concatenation: they reach Swift as single values.
- A label next to a core text (the Stats Today heading, Card Info row labels) is catalog text (I2 or I3) in its own element.

## I5. Collection and outside content (R194)

Shown as stored, never translated, case-changed, re-spelled or normalized for display:

- deck names (`decks/decks` D1): a screen may show the full name or its last component (`docs/design/native-interface-contract.md` N11.5), and may truncate a one-line Mac sidebar row with the full path in accessibility and help (N2); choosing a component or truncating for layout is not a transform;
- tag names, note type, field and template names, preset names, deck descriptions and field content. A deck description is shown through `text/description-html`, which renders Markdown when the deck asks for it and removes unsafe markup, some elements with their content (D2, D3.1); it never translates or re-spells the text it keeps. Field content is drawn by the card pipeline with the note type's own styling;
- stock names: the default deck and preset `Default` (`storage/new-collection`, `decks/deck-presets`) and the stock note types' names, fields and templates (`notetypes/stock-kinds` K1) are created in English and stay English in every later language. Anki names them in its interface language when it creates a collection; Meguri never does, because they are collection data shared with other clients;
- server and add-on texts: a sync server's message (`sync/errors` E1, a parameter by design), shown through `text/description-html`'s sanitizer; text an external tool sends through AnkiConnect or the Migaku server;
- file names, media names, account and device names.

Counts beside a name are never cut to make the name fit (R186).

## I6. Numbers

| Where | Writer | Locale |
|---|---|---|
| Home studied-today line | `stats/studied-today` T5.1 | none: full stop, no grouping (`1234 cards`, `1.5 seconds`) |
| Stats Today lines, Card Info texts | T5.2 | none: en-US, `,` grouping (`1,234`) |
| Answer button intervals, visible form | `sched/answer-labels` A3 | none as specified: full stop, no grouping (I4's open row) |
| Heatmap texts, streak stats, Days learned | `stats/heatmap-presentation` M10.3, `stats/activity-strings` S1, S2 | the user's locale (grouping, percent) |
| Every other number, date, time, percentage and duration in I2 or I3 text | locale formatters (R192) | the user's locale |

- So one screen can mix both: with the region set to Germany and the interface in English, Home shows Daily average with the locale's grouping (`1.234 cards`) and the studied-today line with Anki's digits (`Studied 1234 cards …`). The difference follows R1448 for the line and is not a lint finding.
- The interval's spoken forms (`review/reviewer-strings` S13) put A3's amount in as text (`1.5`), so no locale writes the interval's number anywhere yet. Whether R192's formatters apply to the visible label or its spoken forms is part of I4's open row.

### I6.1 Plural category of a quantity with a fraction

A catalog text that shows a quantity which may have a fraction takes its plural category from the number as the text writes it: the value after rounding to the digits shown, with every fraction digit that stays visible. The locale's CLDR plural rules choose the category from that written number. A screen never takes the category from the unrounded value, and never formats the number at one precision and chooses the form at another.

Examples in English, whose **one** category needs the integer 1 with no visible fraction digits:

| Number | Digits shown | Text | Category |
|---|---|---|---|
| 0.9996 | at most two fraction digits | `1` | **one** (`1 minute`) |
| 1.25 | at most two fraction digits | `1.25` | **other** (`1.25 minutes`) |
| 1.4 | no fraction digits | `1` | **one** (`1 minute`) |

- Other languages apply their own CLDR rules to the same written number, so a language whose **one** category includes some fractions can choose another form: French's **one** covers `1,25`.
- A file whose plural entries vary only on an integer argument, as `review/reviewer-strings` S1's do, cannot pluralize such a quantity directly. Its text either gets an argument already rounded to an integer, or keeps one entry per plural category and picks one by applying the locale's CLDR rules to the formatted number, as S13's spoken units do for English (`.one` for the amount `1` only).
- Core texts (I4) choose their own forms as their files say, for example `stats/studied-today` T5.4.

## I7. The word "Anki" (R156)

U167's lint fails on the letters "Anki", in any case and inside a longer word (`AnkiConnect`, `AnkiWeb`), in user-visible text unless the text is on its allowlist. User-visible text is every catalog value and every Swift string literal that reaches the screen or assistive technology: a view's text, a label, a help tag, an alert, an accessibility label, value or hint, a menu or window title. Launch arguments (`-ShowAnkiServerInterface`, N12.4), identifiers, URLs, log messages, type and file names are not user-visible and are not checked.

The allowlist is Meguri's (U167) and holds plain compatibility facts. It must contain these texts, which the specs and the Product Contract fix:

| Text | Source | Note |
|---|---|---|
| `Support older Anki versions` | `package/transfer-strings` S3, key `export.legacySupport`; the option is `package/apkg-export` A2 and A13, `package/legacy-export` | R1618's and R1625's label; Anki's own label adds ` (slower/larger files)` |
| the Anki Transfers section and its Import from Anki and Export to Anki actions | R118, `docs/design/native-interface-contract.md` N11.4 | hidden while N12.4 holds; the code and its strings stay |
| the Anki Server section | N11.4 | hidden by N12.4; the code and its strings stay |
| the AnkiConnect-compatible server toggle | R153 | the Integrations page (Mac) |
| R1674's port message | R1674 | names Anki as the app that may hold the port |
| R2029's About text | R2029 | the factual compatibility sentence and the independence note |
| R156's own examples ("Syncs with Anki sync servers") | R156 | where Meguri uses them |

- No other I3 key has "Anki" in its English as of 2026-11-10. A file that adds one adds a row here.
- The exact wording of a Product Contract row is the unit's that shows it (I2); the allowlist entry follows that wording.
- The lint never looks at I4 or I5 texts: a deck named `Anki`, a card or a notice that mentions Anki is content.

## I8. Tests

- U167's `lint-strings` fixtures: a catalog entry without a comment fails; a Swift literal `Support older Anki versions` in a view passes and `Open in Anki` fails; `-ShowAnkiServerInterface` as a launch argument passes; a core text passed as a whole `%@` argument passes; `"Studied " + count` in Swift fails.
- Each I3 file's own catalog test, as that file says: an I3 key missing from an existing catalog, or a value that differs from its table, fails.
- The I4 texts are tested where their files say: the studied-today line by oracle observations (`specs/conformance/stats-ops.md` H5, H6); the answer labels' parts by the `answer_labels` observation (`sched/answer-labels` A8), and their visible text by unit tests from A6 and A7; the others by unit tests from their tables.
- U167's catalog tests, fallback and arguments: with the device language set to one Meguri does not ship, every catalog entry resolves to its English value, with its numbered arguments substituted in order (`%1$lld`, `%2$@`). A count entry gives the **one** form for 1 and the **other** form for 0, 2 and 3.
- U167's formatter tests, decimal separator: with the region set to Germany (de-DE) and the interface in English, an I2 text, or an I3 text whose file uses locale formatters (I6), that shows 3.25 writes `3,25`; with en-US it writes `3.25`. An I4 core text that writes the same number (I6 rows 1 and 2) writes `3.25` under both. The locale's separator in English I2 and I3 text is Meguri's choice under R192; Anki's English text always writes a full stop.
- U167's formatter tests, I6.1 in English: 0.9996 shown with at most two fraction digits gives the **one** form with `1`; 1.25 shown the same way gives the **other** form with `1.25`; 1.4 shown with no fraction digits gives the **one** form with `1`.
