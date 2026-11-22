# Stats PDF export

Spec: stats/pdf-export
Requirements: R1421, R1295, R1357, R1360, R1361, R1838
Anki: 26.09.3
Scenarios: none. Anki's Save PDF lives in its desktop Statistics window, which the oracle does not run (`conformance/AGENTS.md`), and the file is not collection data. X11 lists the Meguri tests built from this file.
Status: Approved 2026-11-22 (R1357) for the lines that begin "Changed 2026-11-22 (round 11)". Approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/statistics-round11-20261122.md`); Antoine approved the round 11 design in chat on 2026-11-22 (`docs/design/decisions.md`, rows `stats.overview` of 2026-11-22). Every other section is in force and unchanged.

The file the Stats screen writes when the user exports it (U236): its default name, where it goes, when it is made, what it holds and in what order, and how its pages break. Anki's Statistics window has a Save PDF button that prints its graphs page to a PDF file. R1421 asks for the same export, with Meguri's name prefix. The graphs, tables and their text are those of the screen (`stats/graph-presentation`, `stats/studied-today` T4); this file adds only what the export changes. Where the commands sit is X9's; the rest of the screen is `stats.overview` (`docs/design/screen-inventory.yaml`).

## X1. Terms

Changed 2026-11-22 (round 11): the PDF export is not in the Statistics page's first version (round 11, 9A: `stats/statistics-page` S21). When it is built, this file is rewritten for the page's strip and seven sections; its file name (X2), place (X3) and timing rules (X8) are expected to stay. Until then nothing here is implemented.

- **Box**: one of the screen's thirteen graph boxes, in the order of `stats/graphs-common` G5 (R1357): Today, Future Due, Calendar, Reviews, Card Counts, Review Intervals, Card Stability, Card Difficulty, Card Retrievability, Retention, Hourly Breakdown, Answer Buttons, Added. Card Ease is never one (R1360). Meguri shows the three FSRS boxes whatever the collection's `fsrs` key says (`stats/graph-presentation` P21).
- **Shown result**: the data the screen's model holds for its boxes, with the scope, the history and the scope's display name it was computed for. It is **complete** when the data of all thirteen boxes comes from one refresh (`stats/graphs-common` G8.3), and **partial** while a refresh's first request has returned Data and its second has not, so the boxes below the fold still hold an earlier refresh's data or none.
- **Start instant**: the instant the user chooses an export command (X9).
- **Capture**: the moment the export reads the screen state: the shown result and each box's control values (`stats/graph-presentation` P5). X8 says when it happens. Nothing that happens on the screen after the capture changes the file.

## X2. File name

### X2.1 Default name

The default name is `meguri-stats-<date>@<time>.pdf` (R1421):

- `<date>` is `YYYY-MM-DD` and `<time>` is `HH-MM-SS`: four-digit year, then month, day, hour (`00` to `23`), minute and second, each two digits with a leading zero.
- The values are the local wall-clock time of the start instant in the device's current time zone, with the fraction of a second dropped (09:05:07.900 is `09-05-07`, never `09-05-08`).
- The digits are always ASCII `0` to `9` and the calendar is always the Gregorian one, whatever the user's language, region, numbering system or calendar (a non-Gregorian calendar such as the Japanese one, or Arabic-Indic digits, change nothing). The separators are always `-`, `@` and `-`.
- The prefix `meguri-stats-` is the same in every language (X10).
- The name is fixed at the start instant. A wait before the file is made (X8) does not change it.
- The name carries no zone. In a repeated hour (the end of daylight saving time) two exports an hour apart can get the same default name; the replace question (X3) then applies.

| Start instant | Zone | Default name |
|---|---|---|
| 2026-10-12 07:05:07.900 UTC | Europe/Paris (UTC+2) | `meguri-stats-2026-10-12@09-05-07.pdf` |
| 2026-10-12 21:59:59.999 UTC | Europe/Paris | `meguri-stats-2026-10-12@23-59-59.pdf` |
| 2026-10-12 22:00:00 UTC | Europe/Paris | `meguri-stats-2026-10-13@00-00-00.pdf` |
| 2026-10-25 00:30:00 UTC | Europe/Paris (UTC+2, summer time) | `meguri-stats-2026-10-25@02-30-00.pdf` |
| 2026-10-25 01:30:00 UTC | Europe/Paris (UTC+1, after the change) | `meguri-stats-2026-10-25@02-30-00.pdf` |
| 2026-01-05 03:04:05 UTC | Asia/Kathmandu (UTC+5:45) | `meguri-stats-2026-01-05@08-49-05.pdf` |
| 2026-01-05 03:04:05 UTC | UTC, Japanese calendar (`ja_JP@calendar=japanese`) | `meguri-stats-2026-01-05@03-04-05.pdf` |
| 2026-01-05 03:04:05 UTC | Asia/Riyadh (UTC+3), Arabic-Indic digits (`ar_SA`) | `meguri-stats-2026-01-05@06-04-05.pdf` |

### X2.2 A name the user changes

On Mac and in the iPad export browser the user may edit the name. A name that does not end in `.pdf` (in any letter case) gets `.pdf` appended before the replace question of X3 is asked, so typing `stats` where `stats.pdf` exists asks before replacing `stats.pdf`. `Stats.PDF` stays as it is. The share sheet offers the file under its default name; the destination decides any renaming.

## X3. Where the file goes

- **Mac, File › Export Stats as PDF…** A system save panel opens at once with the default name and the PDF type. Its first folder is the folder this device remembered from its last stats export, or the user's Documents folder (`~/Documents` in the user's home, not the app container's Documents folder of the sandboxed app) when none is remembered or that folder no longer exists.
  - Choosing an existing file asks before replacing it. Declining returns to the panel and writes nothing.
  - The panel's Save remembers the chosen folder, after any replace question is accepted, whatever happens to the export afterwards (X8). Cancel in the panel writes nothing and keeps the remembered folder.
  - The remembered folder is device-local, never in the collection, never synced (R324).
- **iPad, File › Export Stats as PDF…** (the iPad menu bar shares Mac's menus, U159). The system's document export browser opens with the file under its default name. Its first folder is the system's; Meguri remembers none. Cancel writes nothing.
- **iPhone and iPad, the share button.** The share sheet holds the PDF as a file with the default name. Saving, sending or printing it is the system's. Dismissing the sheet writes nothing.
- The file is written whole or not at all. A failed write (disk full, no permission) leaves no partial file at the chosen path, and shows the system's error with its cause. A file it was to replace stays as it was. The screen stays as it was.

## X4. What the file holds, in order

1. **A header**, at the top of the first page only:
   - the title `Statistics`;
   - one line `<scope> · <history>` for the shown result captured (X1), not for whatever the screen's controls say at that moment (X5, scope error). `<scope>` is the deck's full name with `::` between its components for a Deck scope, `Collection` for the Collection scope, or the search text for a Search scope, as the field holds it after `stats/graphs-common` G2 has replaced U+001C to U+001F with U+0020. A Search text that is empty or only white space is the Collection scope (G2) and writes `Collection`. `<history>` is `Last 12 Months` or `All History` (`docs/design/native-interface-contract.md` N11.7). Examples: `Japanese::Verbs · Last 12 Months`, `Collection · All History`, `deck:Japanese is:due · Last 12 Months`.
   - This section defines the line. The screen's compact subtitle (`stats.overview`) has the same form, but its middle truncation never applies here: a long name or search wraps onto more lines.
2. **The thirteen boxes** in X1's order, every one of them, whether or not the user had scrolled to it (X8).

Nothing else: no page header or footer on later pages, no page numbers, no date line (the file name carries the time).

Layout: the header's title uses the system's Title 1 text style in bold, and its line the Body style in the secondary label colour. The boxes take the page's full content width in one column, with the box chrome and the spacing of the screen's one-column layout (R1359: iPhone and narrow windows). The header and the first box share the first page.

## X5. Each box

A box in the file shows what it shows on the screen for the captured state (R1421: "in their current state"):

- its title and subtitle (`stats/graph-presentation` P19, `stats/studied-today` T8), the chart for the selected range and toggles, the table, the memory-state footnote and the Card Counts `Notes` row when the screen shows them;
- the Today box's lines (`stats/studied-today` T4), or its one line `No cards have been studied today.` when there are no answers (T4.1);
- the Calendar for the year shown, with the week start in use (`stats/graph-presentation` P7, `stats/heatmap-presentation` M6);
- Retention's table for the picker's choice (P12), which is always shown: a period with no counted row reads `N/A` and a Count of `0`;
- a graph with no data in its no-data state: `No Data` in place of the plot, with its title, subtitle and control values kept, and no table, except Card Counts, which keeps its table (P16.5, R1361).

**Controls.** Each control the box shows on screen appears as its current value in static text, on one line under the subtitle, in the screen's order, the values separated by ` · `:

| Control | Text in the file |
|---|---|
| A range picker (P5.1) | the selected option's label: `1 Month`, `3 Months`, `1 Year`, `All`, `50%`, `95%`, `100%` |
| Backlog, Time, Separate suspended/buried cards | `<label>: On` or `<label>: Off`: `Backlog: On`, `Time: Off`, `Separate suspended/buried cards: On` |
| Retention's picker | `Young`, `Mature` or `All` |
| Calendar's year | the year as the controls row writes it, with no digit grouping: `2026`, never `2,026` |
| Calendar's "Week starts on" menu (U230, `editor/config-keys` K5) | `Week starts on: <weekday>`, the weekday name the menu shows for the week start in use: `Week starts on: Monday` |

A control the screen does not show is absent: Backlog with no overdue card (P5.1), a range option that needs all history (R1367). Decided 2026-11-10 (proposed, U236): values as text, because a live segmented picker or toggle does not render into a PDF on every platform, and because the file is read, not used.

**Not in the file.** Selections, tooltips and callouts, hover highlights, `Show in Browser`, the Retention help button and its text, scope-error messages, the Back and Forward buttons around the Calendar's year, and any progress indicator.

**Scope error.** While a scope error shows, the screen keeps the graphs it had (`stats/graphs-common` G8.2, R1351). This holds for a Search scope error and for `unknown_deck`, the Deck scope of a deck deleted while it was the scope (`This deck no longer exists.`). The file then holds those kept graphs, and its header names the scope, deck name and history they were computed for: the last search that worked, or the deck's full name as the screen showed it with those graphs. It never names the text that failed. When the screen keeps no graphs (a first open whose first refresh ended in a scope error, such as a remembered search that no longer parses, R1354), the export commands are disabled (X8).

## X6. Pages

- Decided 2026-11-10 (proposed, U236), platform defaults:
  - **Mac**: the paper size and margins of the system's default page setup, the ones a new Page Setup gives on that Mac (A4 or US Letter by the region, unless the user changed the default paper).
  - **iPhone and iPad**, which have no Page Setup: US Letter (612 × 792 pt) when the device's region is one that CLDR's paper-size data gives US Letter (BZ, CA, CL, CO, CR, GT, MX, NI, PA, PH, PR, SV, US, VE), else A4 (595.28 × 841.89 pt). Margins 36 pt on every side.
  - Portrait always.
  - Anki writes A4 portrait in every region, from its web engine's default page layout (X10).
- One column (X4). A box never splits across pages: a box that does not fit in what is left of the page starts on the next page, as Anki's printed page keeps each graph box on one page when it can.
- A box taller than a page's content height (not expected at X7's text size) is scaled down, keeping its proportions, to fit one page's content height. Anki's printed page would split it (X10).

## X7. Appearance and text

- The file uses the light appearance, standard contrast and the default text size (Dynamic Type Large), whatever the device's appearance, contrast and text size settings, so a file reads the same wherever it is opened or printed. Chart colours are the light variants of `stats/graph-presentation` P17.2 and the heatmap palette's light ramp (`stats/heatmap-presentation` M8).
- Text stays text: every title, label, table value and line can be selected, searched and extracted from the PDF. Charts are vector paths, not bitmaps.
- No animation runs while rendering (R1368 does not apply to a file).
- Document information: the title entry is `Statistics`, and the entry naming the application that made the document is `Meguri`. The entry naming the PDF writer is left as the system sets it.

## X8. When the export runs

**Availability.** Each export command (X9) is enabled only when all of these hold; otherwise it is disabled, on Mac and iPad as a disabled menu item and on iPhone and iPad as a disabled share button (the share button stays in place):

- Stats is the focused section (for the menu item; the share button is part of the Stats screen's toolbar);
- the screen holds a shown result, complete or partial (X1). Blank placeholders before a first refresh's first Data, or after a first refresh that ended in a scope error, do not count;
- the screen is not showing its content-unavailable view (an unreadable row, `stats/graphs-common` G8.2);
- no other export from this window is running. A second command while one runs is disabled, not queued.

**Order.**

1. Start instant: the default name is fixed (X2.1).
2. The destination is chosen: Mac's save panel (X3) opens at once, and the capture waits for Save. On iPhone and iPad the capture happens when the share sheet's chosen activity or the export browser asks for the file. Cancel at this step ends the export with nothing written.
3. Capture (X1). When the shown result is complete, the export renders it at once.
4. When the shown result is partial, the export waits until the screen holds a complete result for the same scope and history, then captures again and renders that. The control values are those at that second capture. A refresh that a change set, a rollover or a Context changed result starts while it waits (G8.3, G10) only lengthens the wait: the export takes the first complete result.
5. Rendering, then writing (X3). A refresh after the render begins does not change the file.

**While an export waits or renders** (from step 3 to the end of step 5), the screen's scope, deck, search and history controls and its graph controls are disabled and keep their values, so the user cannot change the state being exported. The screen still scrolls and still refreshes. When the wait and rendering take longer than 0.5 s:

- Mac and the iPad menu item: a sheet on the Stats window shows an indeterminate progress indicator, `Exporting Statistics…` and a Cancel button. Cancel ends the export with nothing written and the controls enabled again. The remembered folder stays as Save set it (X3).
- The share sheet shows the system's own progress while it waits for the file, and dismissing it cancels the export. Meguri adds no indicator of its own.

**A wait that ends without a complete result.** When a refresh for the scope ends otherwise while the export waits, the export ends with nothing written, the controls are enabled again, and an alert `Couldn’t Export Statistics` explains:

| How the refresh ended | Alert message |
|---|---|
| A scope error (a search that now fails, or `unknown_deck` after the deck was deleted, for example by a sync) | the message the screen shows for it (`search/errors` E7, or `This deck no longer exists.`) |
| An unreadable row: the screen shows its content-unavailable view | `Some review data can’t be read. Check Database can repair it.` |
| The collection closed or was replaced, or the window closed | none: the export ends silently with the screen |

A Cancelled result of a superseded refresh does not end the wait: a newer refresh follows, and the export keeps waiting for it.

**What the export computes.** It uses only data in the screen's model; it makes no stats request of its own (`stats/graphs-common` G8) and resolves no scope again. It bins and renders every box, including the ones below the fold the user never scrolled to. This is a deliberate exception to R1838, whose laziness applies to the screen, not to a file. The export emits none of U237's stats signposts (`stats.firstGraphs`, `stats.scopeChange`, `stats.allGraphs`, render or binning signposts), so an export during an interval-definition or performance test does not change what those tests measure. Rendering runs off the main thread where the platform allows. The screen's selection and scroll position do not change.

**Read only.** Exporting never writes the collection (R1295, `stats/graphs-common` G11): no row, `usn`, `mod`, `col.mod` or config key changes. Only Mac's remembered folder (X3) is written, on the device.

## X9. Commands and strings

- **Mac and iPad menu bar**: File › `Export Stats as PDF…`, in the export group after `Export Deck…` (R280). It has no key equivalent, so it takes none of `ShortcutCatalog`'s (U158). It is published by the Stats screen as a focused-scene action (U159).
- **iPhone and iPad**: a share button in the Stats screen's toolbar, with the system's share symbol and label.
- Accessibility identifiers: `stats.export.menu` (the menu item), `stats.export.share` (the share button), `stats.export.progress` (the progress sheet) and `stats.export.cancel` (its Cancel button).

Meguri takes these texts only from this table.

| Where | Text |
|---|---|
| File menu item (R1421) | `Export Stats as PDF…` |
| File header title | `Statistics` |
| File header line | `<scope> · <history>` (X4) |
| Control values (Meguri) | `<label>: On`, `<label>: Off`, `Week starts on: <weekday>`, values joined by ` · ` (X5) |
| Default file name | `meguri-stats-<YYYY-MM-DD>@<HH-MM-SS>.pdf` (X2) |
| Progress sheet (Meguri) | `Exporting Statistics…`, `Cancel` |
| Alert title (Meguri) | `Couldn’t Export Statistics` |
| Alert message, unreadable row (Meguri) | `Some review data can’t be read. Check Database can repair it.` |

Anki's button text `Save PDF` and its notice `Saved.` are not used (X10). The alert's other messages are the screen's own (X8).

## X10. Differences from Anki

| Anki 26.09.3 | Meguri | Why |
|---|---|---|
| Default name `anki-<word>-YYYY-MM-DD@HH-MM-SS.pdf`, where `<word>` is the translation of `stats` in the user's language | `meguri-stats-…`, the same in every language | R1421 fixes the name; one name in every language keeps exports sorted together |
| The save panel opens in the folder last used for stats in that profile, else Documents. The folder is remembered as soon as a file is chosen, even when the replace question is then declined | per device; remembered at Save once any replace question is accepted (X3) | Meguri has no profiles (R324); a declined choice is not an export |
| The `.pdf` extension is appended before the replace question | the same (X2.2) | |
| A transient `Saved.` notice shows as soon as printing starts, before the file exists and even if the write later fails. No write failure is ever reported | no notice; the closing panel or sheet is the confirmation, and a failed write shows the system's error (X3) | native save flows confirm by closing; a lost file must not look saved |
| The page prints whatever has loaded, with no wait, including the loading mark | waits for a complete result (X8) | the file must hold every box's data |
| The page is printed in the current theme, its form controls as drawn | light appearance and controls as text values (X5, X7) | a file that reads the same anywhere; live native controls do not render into a PDF on every platform |
| The top of the page prints the scope radio buttons (with the search box, which reads `deck:current` for the Deck scope) and the history radio buttons. It has no title, and no deck name: the deck chooser is a window control outside the printed page | a `Statistics` title and one `<scope> · <history>` line naming the deck (X4) | the file says what it covers without the window |
| A4 portrait with the web engine's default page layout, in every region | the platform's default paper and margins (X6) | platform defaults; the file is not collection data |
| A graph box taller than a page splits | it is scaled to fit one page (X6) | never split a chart |
| With `fsrs` false: Card Ease, and no Stability, Difficulty or Retrievability. With `fsrs` true: those three and no Card Ease | always the three FSRS boxes and no Card Ease (R1360, R1397) | Meguri is FSRS-only |
| No `Notes` row or memory-state footnote | both, as on Meguri's screen | `stats/graph-presentation` P21 |

None of these changes any golden: the export has no oracle counterpart.

## X11. Tests

Meguri's own (U236), no oracle:

- **Name.** X2.1's table: each start instant and zone gives its name (injected clock, zone and locale), including both rows of the repeated hour, Asia/Kathmandu, the Japanese-calendar locale and the Arabic-Indic digit locale. A start instant of .999 s keeps its second. A 2 s wait before the capture does not change the name.
- **Edited name.** In the Mac panel, `stats` saves as `stats.pdf`; with `stats.pdf` present, it asks before replacing; `Stats.PDF` stays.
- **Content.** With Reviews on `3 Months`, Card Counts with Separate off, Retention on Young, the Calendar on the previous year and the week starting on Monday, the PDF's extracted text (PDFKit) holds the header line, all thirteen titles in X1's order, `3 Months`, `Separate suspended/buried cards: Off`, the Young table, the year and `Week starts on: Monday`. The Reviews table values equal the screen's for 3 months.
- **Lazy boxes.** Export right after opening, before any scroll: every box is in the file, and no stats render or binning signpost is emitted for the boxes below the fold.
- **Empty collection.** The file holds thirteen boxes: Today with `No cards have been studied today.`, Card Counts with its table of zeros, Retention with its table of `N/A` and Counts of `0`, and the other ten in their `No Data` state.
- **Hidden control.** With no overdue card: no `Backlog` text in the file.
- **Header.** Deck scope on `Japanese::Verbs`: `Japanese::Verbs · Last 12 Months`. A Search text with U+001F: written with a space. A white-space-only Search text: `Collection`. A search of 300 characters wraps and is not truncated.
- **Scope error.** A Deck scope, then an invalid search: the header names the deck and its history, and the graphs are the deck's. A deck deleted while it is the scope: the file holds the kept graphs under the deck's old name. Stats opened on a remembered invalid search: both commands are disabled.
- **Waiting.** Export during a refresh's second request: the file appears after the second result, with its data. A change set during the wait: the file holds the newer refresh's complete data. A deck deleted during the wait: no file, the alert with `This deck no longer exists.`. An unreadable row reached during the wait: no file, the unreadable-row alert. Cancel in the progress sheet: no file, controls enabled again.
- **Controls during an export.** The scope, history and graph controls are disabled from capture until the file is written; a second export command is disabled.
- **Pages.** A fixture whose boxes fill more than one page: no box's text is split across two pages; a box that does not fit starts the next page. Mac with the default paper set to US Letter gives Letter pages. iPhone with region US gives Letter, with region FR gives A4.
- **Appearance.** Exporting from a device in dark mode with Increase Contrast and the largest text size gives the same PDF as from a device with default settings (byte comparison of the page content streams, or rendered-image comparison).
- **Document information.** The title entry is `Statistics` and the application entry is `Meguri`.
- **Read only.** `col.mod` and the config table are unchanged after an export (R1295).
- **Mac.** The menu item is disabled when Stats is not the focused section, on blank placeholders and on the content-unavailable view. Cancelling the save panel writes nothing and keeps the remembered folder. Declining the replace question leaves the existing file unchanged. A write to a read-only folder shows the system's error and leaves no file. With no remembered folder, the panel opens in `~/Documents`.
- **iPad.** The File menu item is present in the iPad menu bar and opens the export browser with the default name.
