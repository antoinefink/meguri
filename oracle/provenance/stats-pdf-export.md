# Stats PDF export (U236)

Spec-writer notes for `specs/stats/pdf-export.md`. Session s-20261110T085429Z-165d83d3
(2026-11-10). Read at tag 26.09.3 (`refs/anki-26.09.3`). No code was copied; every
rule was restated in the spec writer's words. The oracle was not run: Anki's Save
PDF is desktop-window behaviour the oracle does not drive.

| Spec section | Source | Read for |
|---|---|---|
| pdf-export X2.1 | `qt/aqt/stats.py` (the Statistics dialog's PDF path), `ftl/core/statistics.ftl` (`statistics-stats`) | the default name: a fixed `anki-` prefix, the translated word for stats, then the local date and time of the moment the button is pressed in `%Y-%m-%d@%H-%M-%S` form (zero-padded, 24-hour, seconds truncated), then `.pdf` |
| pdf-export X2.2, X3, X10 (folder, replace) | `qt/aqt/utils.py` (the save-file helper) | the first folder is the one remembered for stats in the profile, else Documents; the system dialog is told not to confirm overwrites itself; after a name is chosen the extension is appended when the lower-cased name lacks it, then the folder is remembered, then an existing file is replaced only after a yes/no question (so a declined question still updates the remembered folder) |
| pdf-export X4, X6 | `ts/routes/graphs/GraphsPage.svelte` (print media rules), `ts/routes/graphs/RangeBox.svelte` (print media rule and its controls), `ts/lib/components/TitledContainer.svelte` | in print the graph grid becomes one block column; the scope and history bar is printed once at the top instead of staying fixed; each titled graph box avoids a page break inside it |
| pdf-export X10 (printed header) | `ts/routes/graphs/RangeBox.svelte`, `qt/aqt/stats.py` (the deck chooser is created in a window area outside the web view) | the printed bar holds the loading mark, the Deck and Collection radio buttons, the search box (whose text is `deck:current` for the Deck scope) and the two history radio buttons; no title and no deck name are on the page |
| pdf-export X6, X10 (page size) | `qt/aqt/stats.py` (the page is printed with the web engine's default page layout) | the PDF uses the engine's default page, A4 portrait, in every region; the engine's margin default was not opened |
| pdf-export X7, X10 (theme) | `qt/aqt/stats.py` (comment and scroll before printing) | the page is printed in the current theme after scrolling to the top; dark mode can print light text on a white page |
| pdf-export X10 (notice, failures, no wait) | `qt/aqt/stats.py` | printing is started and the `Saved.` notice is shown in the same step, without waiting for the print to finish; nothing reports a failed write; the page is printed as it stands, whatever has loaded |
| pdf-export X10 (FSRS boxes) | `ts/routes/graphs/EaseGraph.svelte`, `StabilityGraph.svelte`, `DifficultyGraph.svelte`, `RetrievabilityGraph.svelte` | Ease renders only when the source data's `fsrs` is false; the three FSRS graphs only when it is true |
| pdf-export X9, X10 | `ftl/core/statistics.ftl` (`statistics-save-pdf`, `statistics-saved`, `statistics-title`) | Anki's button text and its after-save notice (not used by Meguri), and the window title `Statistics` reused for the file header |

Sections not listed (X1, X5 controls as text and the scope-error header, X8
availability, order, waiting, cancel and alerts, X6 platform paper, the scaled
oversize box, the document information, X9 placement and identifiers, X11) are
Meguri's own design for U236, built on `stats/graphs-common`,
`stats/graph-presentation`, `stats/heatmap-presentation`, the `stats.overview`
screen contract, U159 and U237. X6's US Letter region list is CLDR's paper-size
data, not Anki.
