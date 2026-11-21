# Deck list, deck page and deck search

Spec: decks/deck-list
Requirements: R26, R27, R40, R41, R50, R53 (list and page side), R57 (name form), R70, R71, R72, R73, R74, R75, R77, R78, R79, R80, R196, R198, R199, R202, R221, R222, R223, R224, R225, R226, R766
Anki: 26.09.3 (the desktop deck list, deck overview, congratulations page and Study Deck dialog); none for the Meguri rules each section marks
Scenarios: conformance/scenarios/review/finished/

What Anki's deck list, deck overview and Study Deck dialog do that Meguri's Home (U152), deck page (U155), deck management (U156) and deck-list keys (U158) must match, and which parts are Meguri's own. The records and operations are `decks/decks`; the numbers are `sched/counts`; the finished deck's facts are `sched/counts#C7`; their wording is `review/reviewer-strings` S8 and S9; the unbury operation is `sched/manual-ops#M5`; the description pipeline is `text/description-html`. Layout, controls, colours and Meguri's own texts come from the Product Contract and `docs/design/native-interface-contract.md` (N2, N11.5, N12.1, N13.2, N20), never from Anki's page.

## H1. Terms

- **Deck list**: Home's deck tree and the sidebar's deck tree (Anki: the deck browser).
- **Deck page**: one deck's page (Anki: the overview).
- **Finished state**: the deck page's content when the deck has nothing to study now (Anki: the congratulations page shown in place of the overview).
- **Deck search**: Meguri's way to find, open or create a deck by name: the sidebar's search field (Mac, iPad) and the tab layout's Search (`docs/design/native-interface-contract.md` N20.1, N14.2). It replaces the Go to Deck sheet, Meguri's earlier quick switcher (Anki: the Study Deck dialog, opened with `/`).
- **Record**: the deck's `DeckStudyState` (`sched/counts#C7`) for the shown deck at the current instant.

## H2. The deck list (R31, R34, R35, R39, R40, R53)

What Anki's deck list shows, the spec each part comes from, and whether Meguri keeps it. Rows marked "Anki only" describe Anki's page and are not Meguri requirements; Meguri's counterpart is named beside them.

| Part | Anki | Meguri |
|---|---|---|
| rows | one per node of `decks/decks#D12`, nested; a collapsed node hides its descendants | the same |
| order | `sched/counts#C1` (siblings in `unicase` order of their stored names) | the same |
| Default deck | hidden under `sched/counts#C2` | the same |
| counts | `sched/counts#C3` and C4 for every node, filtered decks included | the same values; shown as N11.5 and N13.2 say (non-zero counts only, nothing when all are zero, badges and touch numbers hidden at zero) |
| zero counts | Anki only: shown, in a dimmed style | not shown in deck lists (N11.5, N13.2); the deck page shows zeros in plain secondary text (N12.1) |
| filtered decks | listed with their counts; Anki only: a distinct colour | listed with their counts (`decks/decks#D9`); R53's "distinct style" has no form in N11.5, N12.1 or N13.2 yet (open) |
| current deck | the deck `decks/decks#D11`'s read gives carries a highlight; with `curDeck` missing or naming no deck that is the default deck | R40's mark on the same deck. Its visual form is not in the design contract yet (open). When that deck has no visible row (the default deck hidden under C2, or a deck under a collapsed parent), no row is marked and nothing expands |
| names | the last name component, shown as text: markup in a name is never interpreted (Anki escapes it) | the same |
| disclosure | only a node with child decks has one; it toggles `study_collapsed` (`decks/decks#D10`), and the list keeps its scroll position across the redraw (R36) | the same |

Anki's column headers (Deck, New, Learn, Due) and its count colours apply to its own page only; Meguri's deck rows follow N11.5 and N13.2.

Anki's page also has a studied-today line, a gear menu on each row (Rename, Options, Export, Delete), drag-and-drop reparenting and a bottom bar (Shared, Create Deck, Import File). This spec does not cover them: Meguri's activity section is `stats/activity-strings` and U228 (N11.5); the deck menu, drag and delete are `decks/decks#D6` to D8 and U156; Import File is not on Home (N11.5); Get Decks is H8.

Anki redraws the list in three cases: unconditionally every 10 minutes while the list is shown; at once after an operation that changes study queues, when the main window has focus; and, for such an operation started while the window had no focus, when the window regains focus. An operation the deck list started itself does not mark a redraw. Meguri's refresh triggers are U152's (R45: every collection change, foreground, rollover) plus the next count change (`sched/counts#C6`); Meguri has no periodic redraw (R1854).

## H3. Opening, selecting and starting a deck (R41, R53, R74, R75, R77, R766)

- **Anki.** Clicking a deck's name selects it (writes `curDeck`, `decks/decks#D11`) and then shows its overview. Shift-clicking the row outside the name only selects it; shift-clicking the name selects it and shows the overview, as a plain click does. Study always starts from the overview, with Study Now or the `s` key (H9). Accepting a deck in Study Deck selects it and shows its overview.
- **Selection write.** Opening a deck page and starting a review run D11's select op: it writes `curDeck` only when the value changes, and records no undo step (R42).
- **`activeDecks` (open).** Anki 26.09.3 never writes the `activeDecks` config key after it creates a collection. It computes the studied decks each time it builds the queue (`sched/queue-building#B2`). R41 asks Meguri to write it (the deck plus its descendants), and U152's selection bullet and U157's StudyLauncher repeat that, but this spec and `sched/review-session#C2` step 3 keep the stored value as it is. These cannot all hold. Until the Product Contract decides (amend R41, or specify the value's form and when it is written), implementers keep the stored value, and R41's `activeDecks` clause is not accepted as met.
- **Meguri gestures (Meguri rule).** In the sidebar layout a single click opens the deck page. A double-click, Return on the selected deck or S starts its review (R74). In the tab layout a tap opens the deck page (N14.2, N20.2; this replaces R75's and R766's tap that starts review). The deck menu has no Overview.
- **Nothing to study (R77).** In Anki a deck with nothing to study has no separate overview: its overview is the congratulations page (H5.1). Clicking it in the deck list, pressing `s` on the list or accepting it in Study Deck shows that page at once. Pressing `s` on the page goes to review, which finds no card and returns to the same page, with no notice. The notice "No cards are due yet." appears only when Study Now is clicked on a normal overview whose content has gone stale, so that review then finds no card. Meguri matches the page: a start of a deck whose record has `finished` or `empty` set opens no review, and the deck page shows H5's finished state or H5.4's empty view. Meguri's own part: the stale-overview notice is not used, because a start that finds no card shows the finished state in its place (R77).
- **Filtered decks (R53, Meguri rule).** A filtered deck is never started and has no deck page. A click in the sidebar, a tap, Return or S on it in the deck list, and activating it in the deck search all show R53's explanation (Meguri's text) in place of the deck page, the finished state or review. Its deck menu offers only Rename… and Delete Deck (R53, N20.2). The deck-page keys of H9.2 have nothing to act on.

## H4. The deck page (R70, R71, R72, R73)

Anki's overview of a normal deck, when the deck is not finished, shows from top to bottom:

1. the deck's full human name (`decks/decks#D1`). R70 asks Meguri for the same path as a breadcrumb title. N12.1 makes the title a leading large title and N13.2 pushes subdecks so that Back returns to the parent; whether the large title shows the full path or the last component, with the path shown elsewhere, is open for the design contract. In either form the full human name is the title's accessibility label and its help text;
2. the description, only when it is not empty after rendering: rendered as Markdown when `md` is set, otherwise as HTML (`text/description-html`; Anki inserts the HTML unsanitized, Meguri sanitizes it, R82);
3. the queue counts New, Learning and To Review (`sched/counts#C5`, `sched/queue-building#B9`). Beside each count whose deck-list count differs, the signed difference deck list minus queue, with an explanation on hover or tap (R71, R1345; Meguri's wording, `stats/activity-strings`). It is not a buried count: it can be negative, and cards already buried are in neither count, whatever N11.5 and `docs/design/screens.md` call the badge. Anki writes it with an ASCII sign (`+2`, `-1`); Meguri writes a negative value with U+2212 MINUS SIGN (`+2`, `−1`, R71);
4. Study Now, which has keyboard focus, so Return and Space start it (R73).

Its actions:

| Action | Anki shows it | Meguri | Effect |
|---|---|---|---|
| Options | always, filtered decks included (for a filtered deck it opens the filtered-deck settings) | normal decks only, while U152's registry reports the `.deckOptions` route attached (U232) | the deck's options |
| Rebuild, Empty | filtered decks only | never (`decks/decks#D9`) | |
| Custom Study | normal decks only | never (R436) | |
| Unbury | the deck has buried cards | the record's `sibling_buried` + `user_buried` > 0 (the deck's full subtree by `did`, `sched/manual-ops#M5`) | with both counts above 0, S9's chooser (manual, siblings, all; manual is the default choice); with one kind only, mode all at once, with no chooser |
| Description | normal decks only | normal decks | the description editor (`decks/decks#D14`, R81, R84) |

For a filtered deck Anki shows, in place of the description, a fixed three-sentence explanation of filtered decks (that the deck is for temporary study, that its cards return to their home decks, and that deleting it from the deck list returns them). Meguri has no deck page for a filtered deck (H3).

Anki also shows a "Reviews and Updates" link for a deck whose record carries `sharedFrom` (a deck downloaded from the shared-deck site). Meguri keeps the key (`decks/decks#D13`) and shows no link (H10).

Meguri's deck page also holds parts that Anki's overview does not have. They come from other units and specs, each shown only once its unit attaches it (U152's attach pattern):

| Part | Requirement | Source |
|---|---|---|
| Subdecks list | N12.1 | U155, N13.2 (a subdeck pushes on a stack) |
| deck-scoped heatmap and its four streak stats | R70, R1346 | `stats/heatmap-addon`, U228 |
| Stats action, opening Stats scoped to the deck | R70, R1347 | U228; its text is Meguri's (`stats/activity-strings` S4) |
| Browse link, searching the deck by its full name | R70, R48 | `browser/entry-points`, U210 |
| Last session link, while a persisted session record exists for the deck | R91, R441 | U157, U175 |
| toolbar "+" with the deck preselected | R24 | U195 |

## H5. The finished state (R77, R78, R79, R628)

### H5.1 When it shows

The deck page shows the finished state, in place of the counts and Study, exactly when the record's `finished` is set and its `empty` is not. Anki shows its congratulations page whenever the deck's queue holds no card (an empty deck included, H10).

### H5.2 Paragraphs

S8's paragraphs, keys and order, with these rules for the deck page. A review session has a head; the deck page has none, so the page takes its facts from the record:

| Paragraph | Shown when on the deck page |
|---|---|
| `review.congrats.title` | always |
| ready-in (`review.congrats.nextLearn.minutes` or `.hours`) | `next_learning_ready_at` is present and after now |
| `review.congrats.learnRemaining` | exactly with the ready-in line, after a space in the same paragraph; its number is the count of `learning_later_today` |
| `review.congrats.reviewLimit` | `review_limit_reached`; followed by a Deck Options link, `review.congrats.deckOptionsLink` (`review/reviewer-strings#S8.1`), while the `.deckOptions` route is attached (R78, U152's registry) |
| `review.congrats.newLimit` | `new_limit_reached`; with the same link |
| `review.congrats.buried` with `review.congrats.unburyLink` | `sibling_buried` + `user_buried` > 0; the link runs H4's Unbury |
| deck description | as H4 item 2 |

- `review.congrats.continue` is not shown on the deck page; H5.3 returns to the normal page instead.
- **Ready-in amount.** S8's rule with the instant `next_learning_ready_at`: s = max(60, floor((instant − now) / 1,000)) seconds; minutes below 3,600, hours below 86,400, no line at 86,400 or more; the amount is s divided by the unit's seconds, rounded to the nearest whole number with halves up.
- **Redrawing the sentence.** As S8 requires for the same sentence: a system-driven schedule (a timeline with an entry at each instant the shown amount changes and one at `next_learning_ready_at`) redraws it, never an app timer, and no timer-interval countdown view (`Text(timerInterval:)`, a clock such as 4:59) is used (R437, R1854).
- Anki's page computes its line from the compatibility seconds (`secs_until_next_learn`, the whole named subtree, every queue-1 card) and its limit notices from `review_remaining` and `new_remaining`. Meguri uses the native members above, as S8 does (S11).

### H5.3 Returning to the deck page (R79)

- The page reads the record again at `next_page_change_at` (`sched/counts#C7`), after every change set that touches cards, decks, presets or the config keys the counts read, and when the app returns to the foreground.
- **Scheduling the timed reads.** While the page shows, finished or normal, it holds one one-shot wake at the record's `next_page_change_at`, the deck page's counterpart of Home's single timer (`sched/counts#C6`). That instant covers `next_learning_ready_at` when it is after now, each instant a learning card enters the queue's or the deck list's Learn count, and `next_day_at`. No wake is ever armed at or before now. Each read replaces the wake with one for the new record; leaving the page cancels it. There is no repeating timer and no polling (R1854).
- At `next_learning_ready_at` the card's due instant is now + L. The queue includes it (`sched/queue-building#B6`), so `finished` is no longer set. The page shows H4's normal content with Study enabled. Review never starts by itself.
- **Anki.** Each time Anki's congratulations page fetches its facts (when it loads, and again every 60 seconds while it shows), Anki first checks whether the deck is still finished. When it is not, the main window returns to the overview state and draws the normal overview with Study Now, without starting review. So Anki leaves the page within 60 s after the card becomes ready, and until then keeps the last sentence it drew ("ready in 1 minute" at the latest). Its 10-minute main-window refresh and any operation that changes study queues also redraw the page. Meguri switches at the ready instant itself (H10).

Examples (AE3; scenario `review/finished/ready-in`):

| Learn-ahead L (`collapseTime`) | Only remaining card | Deck page at the start | At |
|---|---|---|---|
| 0 | learning card due in 240 s | finished, "The next learning card will be ready in 4 minutes." | +240 s: normal page, Study enabled, no review started |
| 1,200 s | learning card due in 1,500 s | finished, "… ready in 5 minutes." | +300 s: normal page, Study enabled |
| 1,200 s | learning card due in 1,500 s, at +299 s | finished, "… ready in 1 minute." (s clamps to 60) | |

The wakes for the second row follow C7's examples: the page wakes at +300 s (normal, Learn 1, difference −1), again at +301 s (Learn 1, no difference), and not again before `next_day_at`. A normal page wakes the same way when a later learning card enters Learn: with cards due in 600 s and 3,600 s it wakes at +2,400 s and +2,401 s and at no other instant before `next_day_at`.

### H5.4 An empty deck (R80)

When the record's `empty` is set (`sched/counts#C9`: no card has its `did` or `odid` in the subtree), the deck page shows "This deck is empty" with Add Note (deck preselected, `notes/add-defaults`) instead of the finished state. Anki has no such view: its congratulations page shows for an empty deck too.

## H6. Deck search (R26, R27)

The rules for the results of N20.1's sidebar search and N14.2's touch Search. The Go to Deck sheet is retired (N20.1).

### H6.1 The list

- The entries are `decks/decks#D17`'s snapshot in its order (stored names under `unicase`), with one exception from Anki: the default deck (id 1) is left out when no card has `did` 1. Its children, its name and the deck list's own rule (`sched/counts#C2`) play no part here. Filtered decks are listed (Anki lists them).
- The create row's exact-match check uses every D17 entry, the default deck included (`decks/decks#D18`).

### H6.2 Filtering

A deck is listed when every term of the query occurs in its full human name, ignoring case:

1. Split the query at each U+0020 SPACE. An empty term (from a leading, trailing or repeated space) matches every name. Any other whitespace character is part of its term.
2. Map the full human name (components joined by `::`) and each term to lower case with Unicode's full lowercase mapping, independent of locale (it includes the Final_Sigma rule; U+0130 becomes `i` U+0307). This is not case folding: `ß` stays `ß`.
3. A term matches when its code-point sequence occurs contiguously in the lowered name. A term may span components, because the `::` separators are part of the name.
4. An empty query lists every entry; the deck search then shows the deck tree instead of results (H6.3).

This is Anki's rule and supersedes U156's wording ("a substring of some component"): a term is matched against the whole full name, so it may span `::`, and `GoToDeckFilterTests` follow this section.

Meguri applies NFC to the query before step 2. Stored names are NFC (`decks/decks#D1`). Anki does not normalize the query, which differs only for decomposed input.

| Deck | Query | Listed |
|---|---|---|
| `Japanese::Lesson1` | `ja 1` | yes |
| `Japanese::Lesson1` | `JA les` | yes |
| `Japanese::Lesson1` | `ja 2` | no |
| `Japanese::Lesson1` | `se::le` | yes (spans the separator) |
| `Japanese::Lesson1` | `ja  1` (two spaces) | yes |
| `Straße` | `strasse` | no |
| `Café` | `cafe` | no |
| `日本語::N5` | `n5 日本` | yes |

### H6.3 Rows, highlight and keys

- **Rows.** While the query is not empty: the listed entries in H6.1's order, each its last name component over its parent path joined with " › " (N20.1), then the create row when it shows. An empty query shows the deck tree, not results. The create row shows when the query is not blank by H7's rule (so never for a whitespace-only query) and D18's read finds no deck with the same name; its text is N20.6's "Create deck named “%@”".
- **Highlight (Mac).** After every edit of the query, the first listed entry is highlighted, or the create row when no entry is listed. The highlight does not change the detail.
- ↑ and ↓ move the highlight over all rows, the create row included, and wrap around: ↑ on the first row goes to the last, ↓ on the last to the first.
- Return activates the highlighted row (on iPad, the first row); a click or a tap activates its row (R27). On an entry, Anki selects the deck and shows its overview in every case; Meguri opens its deck page in every layout, compact and sidebar alike, and never starts review (N20.5); a filtered entry shows R53's explanation (H3). The create row opens the New Deck form filled with the query (D18, D19.3). With no row at all, Return does nothing.
- **Empty state.** With no listed entry and no create row (a blank query with nothing listed, or a query whose only same-name deck is the default deck that H6.1 leaves out), the results show a one-line no-match message (Meguri's text). This is the `empty` state of `home.goToDeck`.
- Anki's dialog also has an Add button (Ctrl+N). It pre-fills the create form with the highlighted name, or with the query when nothing is listed, and then opens the created deck. Meguri's create row (R27, `decks/decks#D18`) replaces it and fills the New Deck form with the raw query.
- Anki's Ctrl+1 to Ctrl+9 highlight entries 1 to 9. They are not part of the Product Contract and are not bound (H10).
- While the search field has focus, no single-key shortcut fires (R197).

## H7. Name form (R55, R57, R58, R59, R60, R61)

N20.3's form, with its Name and Parent Deck rows; `decks/decks#D19` composes the full name from them. The "typed text" below is that full name.

- **Create.** Anki's Create Deck pre-fills the current deck's full name. Meguri's New Deck form starts with an empty Name and Parent Deck None (D19.3; H10). Anki strips the typed name at both ends of every character its runtime counts as whitespace, which includes U+001C to U+001F, and creates nothing when the result is empty. Meguri passes the typed text to `decks/decks#D5` unchanged: D1's per-component rule removes the same characters (White_Space by trimming, U+001C to U+001F as control characters), so every name Anki creates is stored the same way. The one difference is a text made only of U+001C to U+001F and White_Space, with at least one of the former: it is not blank for R58, so Meguri creates `blank` where Anki creates nothing (H10).
- **Blank text (R58).** The confirm control is disabled while the Name text is empty or consists only of Unicode White_Space characters. Text such as `::` is not blank: D1 stores it as `blank␟blank`.
- **Existing name (R57).** When D5 returns a deck that already existed (D18's `.added` test), nothing is written and no undo step is recorded. The form closes and opens that deck as it would open a new one: its deck page in every layout, or R53's explanation for a filtered deck. The final-name report below applies.
- **Rename.** Anki pre-fills the full human name (R60). Meguri fills the Name with the deck's last name component and Parent Deck with its parent (D19.3), and disables the confirm control while the composed full name equals the current one (D19.4), so no op is issued and no undo step is recorded. Anki also does nothing for an empty text. Meguri disables the confirm control instead (R58).
- **New Subdeck** (Meguri, R59): a create (`decks/decks#D5`) with an empty Name and Parent Deck set to the deck (D19.3). Anki has no such item. R58's rule keeps the untouched form from creating `<full name>::blank`.
- **Final name (R57, R61).** The form reports the human form of the stored name that D5 or D6 returns when it differs from the human form of D1's conversion of the typed text. So D1's trimming, control-character removal and `blank` alone are not reported; an adopted spelling (R57) or an appended `+` (R61) is. The report is the form's "Saved As" row with the stored name and a Done button; the form stays open until Done, which opens the deck page (N20.3).

## H8. Get Decks (R50)

- The action opens `https://ankiweb.net/shared/decks/` in the default browser, with no query and no Meguri credential. Anki's Get Shared opens the same address.
- It runs no sync, transfer or login code and reads no stored credential. App Store builds do not show it (U152). Which builds show it is the Product Contract's "existing build availability rules".

## H9. Keyboard (R196, R198, R199, R202, R221 to R226)

### H9.1 Anki's bindings in these places

| Anki key | Where | Kind | Effect |
|---|---|---|---|
| `/` | main window | menu | Study Deck |
| `f` | main window | menu | Create Filtered Deck |
| Ctrl+Shift+I, Ctrl+E | main window | menu | Import, Export |
| `d`, `a`, `b`, `t` or Shift+T, `y` | main window | shortcut | Decks, Add, Browse, Stats, Sync |
| `s` | deck list | shortcut | shows the current deck's overview |
| `s` | overview | shortcut | starts review of the current deck |
| Return, Space | overview | focused button | Study Now |
| `o` | overview | shortcut | the current deck's options (for a filtered deck, its filtered-deck settings) |
| `u` | overview | shortcut | Unbury (H4) |
| `c` | overview | shortcut | Custom Study |
| `r`, `e` | overview, every deck | shortcut | Rebuild, Empty of a filtered deck; on a normal deck the request is refused |

- Anki's deck list has no arrow-key navigation and no rename or delete key.
- **Repeats.** Anki's main-window and per-screen shortcuts (kind "shortcut") never auto-repeat: holding the key runs the command once. Menu key equivalents (kind "menu") keep the toolkit's default and do repeat.
- Anki has Korean-layout alternatives only in its reviewer. None exist for the deck list or the overview.

### H9.2 Meguri's bindings

⌘ stands where Anki uses Ctrl (R202). Contexts, registered with U158's shortcut registry:

| Context | Active when |
|---|---|
| `decks.list` | the deck list (Home's deck list or the sidebar's deck tree) has keyboard focus, in any layout |
| `decks.page` | a normal deck's page has keyboard focus (the page or one of its controls), and no text input on it has focus |

Keyboard focus is in one place at a time, so the two contexts are never active together, even when the sidebar layout shows the list beside the page. R200's conflict test lists each context's enabled set separately; a key bound in both (Return, S, O) is enabled in only one at a time.

| Key | Context | Command | Repeats | Source |
|---|---|---|---|---|
| `/` | `decks.list`, `decks.page` | Find… (focus the deck search, N20.1) | no | R26; Anki `/` |
| ⌘F, ⇧⌘O | menu key equivalent | Find… | no | R26, N20.4 |
| ↑, ↓ | `decks.list` | move the selection | yes | R221 |
| →, ← | `decks.list` | expand or go to the first child; collapse or go to the parent | yes | R221 |
| ⌥→, ⌥← | `decks.list` | expand or collapse the deck and all its descendants (`decks/decks#D10`) | no | R221, R38 |
| Return, S | `decks.list` | start review of the selected deck, in every layout (a tap opens the deck page, H3) | no | R74, R222 |
| Space, Return, S | `decks.page` | Study | no | R73, R222; Anki: Study Now and `s` |
| O | `decks.list` (selected deck), `decks.page` | Deck Options | no | R223; Anki `o` |
| ⌥⌘O | menu key equivalent | Deck Options | no | R223 |
| U | `decks.page` | Unbury (H4) | no | R224; Anki `u` |
| ⇧⌘R | menu key equivalent | Rename | no | R225 |
| ⌘⌫ | menu key equivalent | Delete, with R67's confirmation | no | R225 |
| Esc | `decks.page`, iPhone layout | back to Home | no | R226 |

- **Menu key equivalents.** ⌘F, ⇧⌘O, ⌥⌘O, ⇧⌘R and ⌘⌫ belong to one menu command each, not to a context (N20.4). ⌘F and ⇧⌘O work wherever Find… is enabled (N20.4: not during review, not while a sheet or panel owns the keys), also while a text input has focus, as menu key equivalents do (R26 names no exception; R197 limits only single keys). ⌥⌘O, ⇧⌘R and ⌘⌫ act on the selected deck while `decks.list` is active and on the shown deck while `decks.page` is active, and are disabled otherwise, so ⌘⌫ never takes the key from a text input. ⌥⌘O and O are disabled for a filtered deck (R53, O1); ⇧⌘R and ⌘⌫ are not.
- `/` outside these two contexts is U159's global key (`review/commands`); in a text input it types `/` (R197).
- In `decks.list`, Return and S on a selected filtered deck show R53's explanation (H3), and O does nothing.
- S on the deck page follows Anki's overview `s`. R74 names S for the deck list only. Its deck-page use comes from Anki and adds no conflict: no other `decks.page` binding uses S.
- **Repeats.** A command marked "no" runs once per key press and ignores auto-repeat, as Anki's shortcuts do (H9.1). This is Meguri's rule for all of them, including the menu key equivalents, where Anki's repeat. R199 requires it for answers and destructive actions. Here it also keeps a held Space or Return from starting a review and then reaching the review screen. Only the selection-moving arrows repeat.
- **Layouts (R198).** Each single-key binding matches its character first, then the physical key that types it on the US layout. On the Korean 2-set layout the O, U and S keys type `ㅐ`, `ㅕ` and `ㄴ` and still give Deck Options, Unbury and Study.
- Single-key bindings are off while a text input has focus (R197): the deck search field, the name form, the description editor.

## H10. Differences from Anki

These are interface differences. No golden shows them, so `conformance/deviations.yaml` has no entries for them. Meguri's UI tests assert them.

- Meguri never writes `activeDecks`. Anki does not write it either. R41 asks for the write; the conflict is open (H3).
- One click opens the deck page and a double-click, Return or S starts review from the deck list (R74). Anki needs the overview first, and its `s` on the deck list shows the overview.
- Nothing to study: both show the finished page (R77, H3). Meguri's own part is the absence of Anki's "No cards are due yet." notice, which Anki shows only after Study Now on a stale normal overview.
- An empty deck shows "This deck is empty" (R80, H5.4). Anki shows its congratulations page.
- The finished state returns to the normal deck page at the ready instant (R79). Anki returns to its normal overview at the first 60-second re-read after the card becomes ready (H5.3).
- The finished state's notices use C7's native members (S8, S11).
- A filtered deck has no deck page and cannot be started; R53's explanation shows instead (H3). Anki shows its overview with Options, Rebuild and Empty and its own explanation (H4).
- Deck lists hide zero counts (N11.5, N13.2). Anki shows them dimmed (H2).
- Anki redraws its deck list every 10 minutes; Meguri redraws only on its triggers (H2, R1854).
- No Custom Study (R436) and no shared-deck "Reviews and Updates" link (H4).
- Descriptions are sanitized (R82). Anki inserts the HTML unchanged.
- The deck search's create row replaces Anki's Add button (R27). Ctrl+1 to Ctrl+9 are not bound (H6.3).
- The New Deck form starts with an empty Name and Parent Deck None. Anki pre-fills the current deck's name (H7). A name made only of U+001C to U+001F (with or without White_Space) creates `blank`; Anki creates nothing (H7).
- New Subdeck, the Parent Deck row and the final-name report are Meguri's (H7, D19).
- No deck-list key auto-repeats except the selection arrows. Anki's menu key equivalents (`/`, Import) repeat (H9).
- Delete asks first when cards would be removed (R67). Anki deletes at once and offers undo.

## H11. Tests and scenarios

The oracle runs Anki's collection library, not its deck list, overview or dialogs. The finished deck's facts are collection facts, and the scenarios below record their compatibility side. Everything else is tested by Meguri:

| Test (unit) | Covers |
|---|---|
| `GoToDeckFilterTests` (U156) | H6.2's table, Final_Sigma and U+0130, NFC of the query, the empty query; H6.1's default-deck rule (empty id 1 with a child: left out; id 1 with one card: listed; id 1's card sitting in a filtered deck with `odid` 1 and `did` elsewhere: left out) and filtered entries listed; H6.3's rows (no create row for a blank query, the tree for an empty one), highlight after an edit (the create row when nothing is listed), the wrap-around over the create row, Return with no row, and the empty state |
| `DeckPageStoreTests` (U155) | H4's actions and their conditions; H5.1 and H5.4's choice from `finished` and `empty`; H5.2's paragraphs from a record; H5.3's examples with an injected clock; the reads at each H5.3 trigger; one wake at a time at `next_page_change_at`, cancelled on leaving: a single finished card that splits the queue's and the list's Learn instants (+300 s, +301 s), a normal page that a later card enters Learn on, and a whole simulated day with at most one wake pending at any time and none at or before now |
| `DeckActionsModelTests` (U156) | H7: blank detection (spaces, U+3000, `::` not blank), New Subdeck's untouched form disabled, no op for an unchanged rename, D19.7's cases, an existing name opening that deck with no undo step, the final name report (reported for `+` and adopted case, not for trimming) |
| `KeyMatcherTests`, `ShortcutConflictTests` (U158) | H9.2's two contexts and their enabled sets, the menu key equivalents' targets and their disabling in text inputs, the repeat column, the Korean physical-key cases for O, U and S |
| `HomeStoreTests` (U152) | H2's current-deck mark with `curDeck` missing or naming a deleted deck, and no mark for a hidden Default or a deck under a collapsed parent; H3's single select write; a filtered deck showing R53's explanation for click, tap, Return and S |

Scenarios (inactive until recorded):

| Scenario | Covers |
|---|---|
| `review/finished/ready-in` | H5.3's examples: the compatibility seconds and the queue at the start, one second before readiness and at readiness, with `collapseTime` 1,200 and 0. New over the recorded `sched/queue/learn-ahead/ready-in` (which already shows readiness at L = 1,200 and the switch to `collapseTime` 0): the 60 s clamp one second before readiness, and entry into the queue at the exact due instant with L = 0 |
| `review/finished/learning-window` | the compatibility facts H5.2 sets aside: queue-1 cards due at `next_day_at` and after it both count in `learning_remaining`; `secs_until_next_learn` comes from the earlier one and clamps to 60 at readiness; after the rollover the earlier card, still in queue 1 and due at that instant, keeps it at 60 (C7's minimum over every queue-1 card, overdue ones included) |

## H12. Built in round 10 (Meguri rule)

Changed 2026-11-21 (round 10; `docs/design/native-interface-contract.md` N24.3, N24.4; Antoine's answers 1A, 2A and 7A). In force: approved by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs, after independent review (`docs/process/review-log/round10-today-20261121.md`).

- **The sidebar's double-click and keys** (H3, H9.2): built for the sidebar layouts' deck list. A double-click (the Mac, and the iPad with a pointer; touch has no double tap) on a deck row, or Return or S while the list has keyboard focus and a deck row is selected, start that deck through the deck page's Study path and its availability (`docs/design/native-interface-contract.md` N14.3). The first click of a double-click opens the deck page, as a click does. A deck with nothing to study shows what H3 says (its finished state or H5.4's empty view); a filtered deck shows R53's explanation. Each runs once per press (H9.2's repeats).
- **Today's keys.** In the sidebar layouts, Return and S start Study (the walk of N24.3) while the sidebar has keyboard focus with its Today row selected, or while Today's page has focus; they do nothing while nothing waits or while Study is unavailable. This adds a row to H9.2's table: Return, S | `decks.today` (the Today row selected in the sidebar, or Today's page focused) | Study All Decks | no | 1A. `decks.today` is never active with `decks.list`'s deck selection or `decks.page`; H9.2's layout rule (R198) and R200's conflict listing apply.
- **The row's number** counts new + learning + to review (`sched/counts`), hidden at zero; its help tag gives the path and the three counts (N24.4).
- **The walk** (N24.3) is a sequence of ordinary starts of `sched/review-session`, one per deck, each with H3's selection write for its deck. It writes nothing else and changes no scheduling.
