# Statistics page (round 11)

Spec: stats/statistics-page
Requirements: R1290, R1291, R1294, R1295, R1349, R1353, R1356, R1363, R1365, R1368, R1397, R1398, R1403, R1405 to R1409, R1417 to R1420, R1439 to R1445, R1837, R1838; supersedes in part R1350, R1352, R1354, R1355, R1357 to R1359, R1361, R1362, R1364, R1366, R1367, R1369 to R1372, R1375 to R1377, R1379 to R1383, R1386 to R1389, R1391, R1392, R1394 to R1396, R1399 to R1401, R1410, R1412, R1414 to R1416, R1421 (S18 and the plan's §5 list them)
Anki: none (Meguri design, decided by Antoine on 2026-11-22, round 11: 1A 2A 3A 4A 5A 6A 7A 8A 9A, with All as the first period and Forgotten in orange; the loading rule of S10 decided the same day). The statistics it draws keep their own specs and Anki rules (`stats/graphs-revlog`, `stats/graphs-cards`, `stats/history-reads`)
Scenarios: none; tests named in S20
Status: Under review (R1357). Do not implement until approved.

The behaviour of the Statistics page as round 11 decided it: one dense page of seven sections in a fixed order under a strip of today's figures, one deck filter and one period for the whole page, every chart in one system, each chart's own page, Show in Browse, the states, tiered loading with kept results, live refresh, and the speed targets. Layout and presentation are `docs/design/native-interface-contract.md` N25; the screen contract is `stats.overview` in `docs/design/screen-inventory.yaml`. The data are `stats/graphs-common` G8 requests with the parts of `stats/history-reads` H5. Where `stats/graph-presentation` or `stats/graphs-common` differ, this file wins for the Statistics page, and those sections carry a "Changed 2026-11-22 (round 11)" note (`stats/graph-presentation` P24).

## S1. Terms

- **Page scope**, **day**, **date of a day**, **period window**: `stats/history-reads` H1.
- **Box**: one chart or table with its header, figure, plot, legend and figures (N25.4). **Lead box**: the first box of a section, which spans the row.
- **Charts of the past**: boxes with a time axis that looks back (Knowledge Over Time, Retention, Forgotten, Reviews Over Time, Cards Over Time, Added and Started). **Charts of the present**: Card Counts, Retrievability, Stability, Difficulty; their header says "Now" and the period never changes them. Reviews Due looks ahead. Activity shows every year. Answers, Time of Day and By Deck have no time axis and use the period window.
- **Kept result**: the last complete or partial result the page showed for a (collection, page scope, period), S10.2.

## S2. One meaning of "remembered"

On this page a review is remembered when `stats/graphs-revlog` V4.1 counts it and its `ease` is not 1: a review of a card whose last interval was a day or more, or any review row, answered with anything but Again, and not cramming. Retention, Forgotten (its fails), Time of Day's upper panel (`stats/history-reads` H7) and the strip's Retention Today all use it. Answers counts every press, learning steps included, so it shows counts and shares and no retention figure. Anki's Hourly Breakdown "% correct" and Answer Buttons "correct" (`ease` above 1 of other rows) are not shown.

## S3. The two controls

### S3.1 Deck

- A pop-up button showing "All Decks" or the deck's name. Its menu: All Decks first, then the deck tree indented in the deck list's order, filtered decks last.
- A deck includes its subdecks, as study does (`stats/graphs-common` G2.1). The subtitle says so: "<n> cards in <m> decks" (Card Counts' total for the scope, and the decks of the scope, the deck with its subdecks or, for All Decks, every deck, filtered decks included; "1 deck" for one), S10.4.
- Choosing a deck here never changes the current deck or what Study uses (R1353): it writes nothing (S17).
- A row of By Deck (S7.15) also sets it.

### S3.2 Period

- One segmented control: 1M, 3M, 6M, 1Y, All (accessibility labels "1 Month", "3 Months", "6 Months", "1 Year", "All").
- It sets every chart of the past, Reviews Due's distance ahead (S4.2) and the windows of the charts with no time axis (S4.3). Charts of the present and Activity do not change.
- 1M to 1Y use G3's `last12Months` history, All uses `allHistory`. Changing among 1M to 1Y re-bins in Swift with no core call, Knowledge Over Time included (its samples are already there for every day of the year, `stats/history-reads` H3.2); a change to or from All is a new refresh (R1838).

### S3.3 Where the state is kept

- The deck and the period are kept per window (scene state) and per device, and the next Statistics page opened on that device starts with the last used ones. They are never synced and never written to the collection.
- The first time on a device, the deck is All Decks and the period is **All** (decided by Antoine, 2026-11-22: "All not 1y is the default").
- Opening from a deck (S13) replaces the deck and keeps the period.
- Each box's own switch is kept the same way, per device: Reviews Over Time's By Type or By Answer and Count or Time, Retention's All, Young or Mature. A stored value that is not an option reads as the first option.
- A kept deck that no longer exists opens on S12.6's state.

### S3.4 Anki's synced display keys

`firstDayOfWeek`, `cardCountsSeparateInactive` and `futureDueShowBacklog` (`stats/graphs-common` G7) are still read by the core and carried in the result, and the page never writes them. Their effects are fixed instead: weeks start on the system's first weekday (S4.1), suspended and buried cards are always their own rows (S7.7), and overdue cards always show (S7.9). Another Anki client keeps its own settings.

## S4. Bins

### S4.1 Time axes of the past

| Period | Bars | Each bar |
|---|---|---|
| 1M | 30 | one day, days −29 to 0 |
| 3M | 13 | one week: the current week (its days up to day 0) and the 12 before it |
| 6M | 26 | one week |
| 1Y | 52 | one week |
| All | one per calendar month, from the first month (below) to the current month | one month |

- A week is seven consecutive dates starting on the system calendar's first weekday for the app's locale. Weeks and months group days by the date of each day (`stats/history-reads` H1). The current week and month end at day 0.
- **All's first month** is the month of H9's `oldest_day` (the scope's oldest review) for every chart of the past, Knowledge Over Time, Cards Over Time and Added and Started included, so the charts of a section share one axis. With no review, it is the month of the oldest creation day of a scope card, but not before January of the year ten years before the current one (card ids written in seconds, or negative ids, would otherwise give centuries of empty months). Counts dated before the first month go into the first bar, and levels (Cards Over Time) start from their value at its start.
- **Rows after today.** A row or card whose day is 1 or more (clock skew, `stats/day-buckets` B3) counts in today's bar in every chart, Reviews Over Time and Added included, as `stats/history-reads` H1 folds the new reads.
- A bar's value is the sum over its days (counts, times, retention counts), or the value at its end for levels (Cards Over Time, Knowledge Over Time): the end of its last day, and for the current bar the value now.
- At most 52 bars except All, which can have more on an old collection; their width follows the plot.

### S4.2 Reviews Due, ahead

Overdue cards (`future_due` keys below 0) are one bar on the left, labelled "Overdue", shown only when there are some. Then, from today: 30 days for 1M; 13, 26 or 52 weeks starting with the current week for 3M, 6M and 1Y; for All, months from the current one to the later of the 12th month and the last month with a card due, but never past the 24th month; cards due after the 24th month go into a last bar labelled "Later". Each bar sums `future_due` over its days (k = 0 is today).

### S4.3 Windows of the charts with no time axis

Answers, Time of Day and By Deck use `stats/history-reads` H1's period windows: the last 30, 90, 182 or 365 days, or all history. They are fixed numbers of days, not the weeks the time axes show; a chart's page says "the last 90 days".

## S5. Sections

The page, top to bottom: the strip (S6), then seven sections, each a heading with its title and its question in secondary text, then its boxes. The lead box spans the row; the others follow side by side (N25.3).

| Section | Question | Boxes, in order |
|---|---|---|
| Knowledge | How much you know, and how much you forget | Knowledge Over Time (lead), Retention, Forgotten |
| Reviews | How you review over time | Reviews Over Time (lead), Activity |
| Cards | What your collection is made of, and how it grew | Cards Over Time (lead), Card Counts, Added and Started |
| Coming Up | What is waiting for you | Reviews Due (lead) |
| Answering | Your answers, by kind and by hour | Answers, Time of Day |
| Memory | How stable recall is now | Retrievability, Stability, Difficulty |
| Decks | Where your work and time go | By Deck (lead) |

View › Go to Section (Mac) and the Jump menu (iPhone, the folded Duo) list the seven and scroll to one (N25.6, N25.11).

## S6. The strip

Five figures with no box, each a value with its unit in small text; a click or tap on one scrolls to its section. They follow the deck filter.

| Figure | Value | Empty | Section |
|---|---|---|---|
| Studied Today | `stats/history-reads` H12's `cards` as "<n> cards" and `time_ms` as a duration ("19 min") | "None yet" | Reviews |
| Retention Today | V4's `today` period over young and mature: passed ÷ (passed + failed), one decimal | "—" until a review counts | Knowledge |
| Remaining Today | the scope's cards waiting today (new + learning + to review: the deck list's counts, `sched/counts` C5; All Decks: Today's totals, N14.1), then "· <n> tomorrow" with C3.6's due tomorrow | "None" | Coming Up |
| Streak | S7.5's current streak, "<n> days" | "0 days" | Reviews |
| Mature | Card Counts' mature count, "of <total>" | "0" | Cards |

- On the iPhone and the folded Duo the strip has four tiles; Mature is a figure of Card Counts instead.
- Studied Today here and the Today page's Studied Today row read the same numbers for All Decks (`stats/studied-today` T12.1), so they agree. The strip replaces Anki's Today sentences (`stats/studied-today` T4): it says what was remembered instead of an Again count.
- VoiceOver: each figure is one element, "Studied today, 128 cards, 19 minutes".

## S7. The boxes

Each box: its header (the title and a chevron, a button to its page, S9), one large figure with its sentence, the plot, the legend in stack order, then two to four small figures. Shared rules are S8. The data members are `stats/history-reads` H5's parts. Numbers are written by the system's formatters (S8.7).

### S7.1 Knowledge Over Time (lead)

- Data: `knowledge_over_time` (H3), sampled at each bar's end (S4.1).
- Plot: two lines on one count axis, no fill: Known (the knowledge sum, green) and Studied (grey, dashed). The y axis starts near the lowest value, so a year's change is visible; because it does not start at zero nothing is filled.
- Header: the title with "Estimated" in secondary text.
- Figure: Known now, "<K> cards" (the replay at now, rounded to the nearest card); sentence: "<p>% of the <S> cards with an estimate" and "up <n> since <date>" or "down <n> since <date>", from the end of the day before the first bar. It is not Retrievability's Estimated total knowledge, which sums the stored memory states of every card (S7.12); the chart's page says how the two differ.
- Figures: Studied, Likely Forgotten (S − K, with its share), Change in Period (since the end of the day before the first bar).
- Callout: the period, Known, Studied, and the gap as "Likely forgotten <n> (<p>%)".
- No Show in Browse: a past state cannot be listed.
- Empty: no review ever, "No reviews yet" and "This fills in as you study."; reviews but no card with an estimate, "No cards with a memory estimate".
- A footnote when H3's `left_out` or `failed` is above 0: "<n> suspended cards are not included." and "<n> cards could not be estimated."

### S7.2 Retention

- Data: `retention_by_day` (H4), summed per bar; the switch All, Young or Mature picks the counts.
- Plot: a line with points (green), and a dashed rule at the desired retention (H3.4) in the label colour, named in the legend "Desired retention". The axis runs from 70% to 100% and extends down when a bar is lower; its labels show the range.
- A bar with fewer than 20 counted reviews is an open point, left out of the line ("An open point has too few reviews to trust", in the chart's page); a bar with none breaks the line.
- Figure: the period's retention (one decimal); sentence: "Desired retention <d>%" ("Average desired retention" when `several`).
- Figures: Remembered, Forgotten, Young, Mature (the last two as retention).
- Callout: the period, retention, remembered and forgotten counts, the desired retention.
- No Show in Browse (it counts reviews over a span; Forgotten has it).
- Its page adds Anki's retention table as "By Period" (`stats/graph-presentation` P12: Today, Yesterday, Last week, Last month, Last year, and All time with All) with its Young, Mature and All modes and its help text.

### S7.3 Forgotten

- Data: `retention_by_day`'s fails (young + mature), per bar.
- Plot: bars from zero, **orange** (decided 2026-11-22: Antoine found red "a bit aggressive"; Again keeps red only inside the answer colours).
- Figure: forgotten reviews in the period; sentence: "<n> a day on average" (over the period's days).
- Figures: Young, Mature.
- Show in Browse: a single day's bar (1M), "Cards Forgotten on <date>" (`stats/browse-queries` Q9).

### S7.4 Reviews Over Time (lead)

- Data: `reviews` (V2) with `count_by_answer` and `time_by_answer` (H6), summed per bar.
- Switches: By Type or By Answer; Count or Time (on the iPhone, Count or Time is in the box's menu).
- By Type: stacked bars from the baseline Mature, Young, Relearning, Learning, then Filtered only when the period has filtered rows (`stats/graphs-revlog` V1's kinds). By Answer: Again, Hard, Good, Easy from the baseline, in that fixed order.
- Figure: reviews in the period (or the time); sentence: "<n> days in a row" when the current streak (S7.5) is above 1, else "<d> days studied".
- Figures, Anki's table lines (`stats/graph-presentation` P8's meanings, rewritten by S8.7): Days Studied "<a> of <b>", Average a Day (over the period), Average on Days Studied, Time per Card (with Time).
- Callout: the period, every segment with its value, the total. The selected bar keeps its colour; the others fade.
- Show in Browse: a single day's bar, "Cards Reviewed on <date>". Weeks and months have none.
- With By Answer, unrated rows (H6 `unrated`) are left out of the bars; the chart's page says how many.

### S7.5 Activity

- Data: `activity_days` (H11).
- Grid: Today's heatmap extended (N24.6's squares, green and AH7's level rule, AH9's month labels): one block per calendar year, the current year first and every earlier year back to the scope's first review. AH7's levels are computed per block, from the days that block shows. The week starts on the system's first weekday. The period does not change it.
- Beside the grid: Current Streak, Longest Streak, Days Studied This Year, and each year's total ("2025: 41,233 reviews").
- **Streaks** (Meguri's own; `stats/heatmap-addon`'s streak rules are not used): a day is studied when its H11 count is above 0. The current streak is the number of consecutive studied days ending today, or ending yesterday while today has none yet (a day not yet studied does not break it). The longest is the longest such run in all history. Days follow Anki's day (`stats/history-reads` H1, the collection's rollover hour), so a review just after midnight before the rollover counts for the day before, and travel across time zones can move a day.
- Selection: a day shows its date and count; Show in Browse for a day with reviews, "Cards Reviewed on <date>" (H13 `answered`).
- On the iPhone and the folded Duo the year blocks are one horizontal strip of weeks that swipes back through time (N25.6), not stacked blocks.
- No streak appears on the Today page, which keeps its one small grid (N14.11, N24.6; round 10 kept the streak off Today, and round 11's 7A puts it here only).

### S7.6 Cards Over Time (lead)

- Data: `card_states_over_time` (H2): each bar's end value.
- Plot: a stacked area from zero with a thin separating line between bands, from the baseline Mature, Young, Relearning, Learning, New.
- Figure: the cards now (the five states' total); sentence: "<m> mature, up <n> in the period".
- Figures: New, Learning (Learning and Relearning), Young, Mature, each with its change since the end of the day before the first bar.
- Footnote when `left_out` is above 0: "<n> suspended cards are not shown." Buried cards are shown in their state. The last bar equals Card Counts' study rows with the buried cards added back by state (`stats/history-reads` H2.2).
- Callout: the date, the five states and the total.
- No Show in Browse for the past; today's states open from Card Counts.

### S7.7 Card Counts

- Data: `card_counts`' `excluding_inactive` set (C4.1), always: suspended and buried are always their own rows.
- Plot: one horizontal bar divided by state (no donut), then the table: a row per state, Mature, Young, Relearning, Learning, New, Suspended, Buried, each with its swatch, name, count and share of the total; a row with 0 stays, in grey. Then Total and Notes (C4.2).
- Figure: "<n> cards in <m> notes". On the iPhone and the folded Duo a figure Mature ("<n> of <total>") is added, since the strip has four tiles there (S6).
- Each state row is a button that opens Browse on that state (`stats/browse-queries` Q4.1's text for the row, Q9).
- It always shows its table, zeros included.

### S7.8 Added and Started

- Data: `added` (C10) and `started` (H2.3), per bar.
- Plot: bars for cards added (blue, the colour of New) and a line for cards studied for the first time (orange, the colour of Learning). Both are counts of cards on one axis.
- Figure: cards added in the period; sentence: "<s> started".
- Figures: Added a Day, Started a Day.
- Show in Browse: a bar of Added opens the cards added in its days (Q9).

### S7.9 Reviews Due (lead)

- Data: `future_due` with `future_due_young` (H8), per bar (S4.2).
- Plot: stacked bars from the baseline Mature (green) and Young (mint); the Overdue bar grey, labelled on the plot, with its count.
- Figure: due in the shown span; sentence: "<r> remaining today · <t> tomorrow" (S6's values).
- Figures: Overdue, Due Tomorrow, Next 30 Days, Daily Load (C3.4).
- Callout: the period, Young, Mature, and "Total by then": the running total from today (no second axis).
- Show in Browse: any bar, including weeks and months and Overdue (Q9).
- The chart's page says it is the schedule as it stands: it adds no new cards and no forgotten cards.

### S7.10 Answers

- Data: `buttons` (V6.2) for the period window (S4.3), `six_months` from H7.
- Plot: one full-width bar for each of Learning, Young and Mature, divided Again, Hard, Good, Easy in that order with a gap between segments; the table under it repeats the four counts as columns per group.
- Figure: presses in the period; sentence: "<p>% Good" (the largest share's button).
- Figures: the share of Again for each group.
- Callout: a segment gives its count and share of its group. No Show in Browse.

### S7.11 Time of Day

- Data: `hours` (V5.3) with `retention_by_hour` (H7) for the period window.
- Two aligned panels on one clock, sharing the x axis and never a y axis: above, the share remembered per hour (points and a line, green), only for hours with at least 20 counted reviews; below, reviews per hour (bars, indigo; V5's `total`). The clock follows the system's 12- or 24-hour setting; on compact widths only even hours are labelled.
- Figure: "<h> is your busiest hour"; figures: Best Hour for Remembering (among hours with 20 or more), Reviews Before Noon.
- Selection: one rule through both panels. No Show in Browse.

### S7.12 Retrievability

- Data: `retrievability` (C9): bars of 5% from the 1% keys (keys of 100 and above in the last bar), green, with a dashed rule at the desired retention.
- Figure: Average retrievability (C9.4, whole percent); sentence: "Estimated total knowledge <c> cards, <n> notes" (C9.4's sums over the stored memory states, rounded; not Knowledge Over Time's replay, S7.1).
- Figures: Below Desired Retention (cards), Cards with a Memory State.
- Every bar opens its cards (Q9). Header "Now". Footnote: C6.2's left-out count (`stats/graph-presentation` P19's footnote).

### S7.13 Stability

- Data: `stability` (C7). Bars that double, on the rounded stability key k (days): under 1 day [0, 1), 1–3 days [1, 3), 3–7 days [3, 7), 1–2 weeks [7, 14), 2–4 weeks [14, 28), 1–3 months [28, 90), 3–6 months [90, 180), 6–12 months [180, 365), 1–2 years [365, 730), over 2 years [730, ∞). Indigo.
- Figure: Median stability (Q(0.5) of `stats/graph-presentation` P4.5 over the map, as a duration); figures: Median Interval (P10's Median interval over `intervals`, C5: where the Review Intervals box went), Over 1 Year.
- Every bar opens its cards (Q9). Header "Now". Footnote as S7.12.

### S7.14 Difficulty

- Data: `difficulty` (C8): ten bars of 10% from the 1% keys (keys of 100 and above in the last bar), indigo, from easy to hard.
- Figure: Median difficulty (C8.2, whole percent); figures: Top Tenth (cards in the last bar).
- Every bar opens its cards (Q9). Header "Now". Footnote as S7.12.

### S7.15 By Deck (lead)

- Data: `by_deck` (H10) for the period window.
- A table: name, cards, reviews in the period, retention (passed ÷ (passed + failed), one decimal, "—" with none), and the share of study time as a number and a small bar (indigo). Sortable by any column on the Mac and the iPad. On the iPhone: the name, retention and the bar.
- Figure: "<p>% of your time in <deck>" (the largest share).
- A row's click (tap) filters the page to that deck: the deck pop-up changes with it (S3.1). The own-cards row (H10.1) does not filter: it is the deck shown.
- A deck with no subdecks: one line, "This deck has no subdecks". The table's rows count a card in a filtered deck in its home deck's row (H10.1), so they add up; the other boxes follow G2.1.

## S8. Shared chart rules

### S8.1 Colours

Two fixed families, system colours with their light, dark and Increase Contrast variants (no custom values):

| Family | Members |
|---|---|
| Card states | New blue, Learning orange, Relearning red, Young mint, Mature green (the heatmap's system green, N24.6), Suspended yellow, Buried grey; Filtered purple only in Reviews Over Time › By Type |
| Answers | Again red, Hard orange, Good green, Easy blue |

- Exception, with its need: these are chart series colours (`ChartColors`), not the count colours of the rubric's `color-meaning` rule (New blue, Learn red, Due green for deck counts; Hard and Easy neutral for the answer buttons, R32, R355), which stay where they apply. Stacked series need a distinct colour for each member, Anki users know these chart colours (R1363 already gives Learning orange and Relearning red), and Antoine approved both families with round 11. Verification: the critic is told of this exception; the VM captures show each family in light, dark and Increase Contrast.
- A chart shows one family, never both. A chart of a single measure uses indigo (Stability, Difficulty, Time of Day's reviews, By Deck), except Forgotten (orange), Retention, Retrievability and Knowledge's Known line (green), and Studied (grey, dashed).
- Every chart has a legend in stack order and a callout that names each value, so colour is never the only cue (R1443). With Differentiate Without Colour, stacked series get a separating line.
- Replaces `stats/graph-presentation` P17.2 and R1363's Young light green and Mature dark green (now mint and green).

### S8.2 One axis

No chart has a second y axis. A running total is in the callout (Reviews Due's "Total by then"); where the total is the story it is its own chart (Cards Over Time, Knowledge Over Time). Time of Day is two aligned panels. The y axis labels are on the trailing side, as Swift Charts draws them. Replaces P17.1 and R1364.

### S8.3 Selection

- Mac: the pointer over a chart shows a vertical rule and a callout; a click pins it; Esc or a click outside unpins.
- Touch: touch and hold, then drag along the chart; a plain vertical swipe still scrolls the page.
- The callout names the period (the date, "Week of <date>", or the month and year), lists every series with its swatch, name and value, and ends with Show in Browse when the item can be listed (S8.4). Changing a switch or the period clears the selection.
- Keyboard: S14.

### S8.4 Show in Browse

- Opens Browse in the same window with the deck and the item as filter tokens (`stats/browse-queries` Q9); Back returns to the Statistics page at the same scroll position and selection (R1419). On the Mac a double-click on a bar does it directly.
- Charts that count reviews offer it for a single day only: Reviews Over Time, Forgotten and Activity, with 1M's day bars or Activity's days.
- Offered by: Forgotten, Reviews Over Time, Activity, Card Counts' rows, Added (Added's bars), Reviews Due, Retrievability, Stability, Difficulty. Never by Knowledge Over Time, Cards Over Time, Retention, Answers, Time of Day or By Deck.

### S8.5 Empty and sparse

- A box keeps its place and its size in every state.
- With no data it shows its title and one line in the plot area that says why: "No reviews yet" (no review ever in the scope), or "No reviews in the last month" with the period ("the last 3 months", "the last 6 months", "the last year"), "No reviews in <deck> in the last month" with a deck filter, "No cards due" (Reviews Due), "No cards with a memory state" (Memory boxes), "No cards with a memory estimate" (Knowledge Over Time).
- With fewer than three bars holding data, bars keep a fixed width instead of filling the plot, and a line shows its points.
- Replaces `stats/graph-presentation` P16.5's "No Data" state for this page.

### S8.6 Large collections

The core returns counts per day or per bin, never rows (`stats/graphs-common` G6, `stats/history-reads` H2.4): 150,000 cards and 1.2 million reviews draw as many marks as 400 cards.

### S8.7 Numbers

The system's formatters in the app's locale: grouped counts, durations in the short style ("19 min", "1 h 12 min"), percentages with one decimal, dates in the short style ("17 Nov", "17 Nov 2025" outside the current year). `stats/graph-presentation` P2's writers and P3's day labels apply to no text on this page.

## S9. A chart's own page

- A box's header is a button (the title and a chevron). It opens the chart's own page in the same column: the chart at full width with the same switches, every row of its table (one row per bar: the period and each series' value), "About This Chart" in two or three sentences, and a link to the setting behind it when there is one (Desired Retention: Deck Options).
- The page keeps the toolbar's deck and period working. Back: ⌘[ or the toolbar's back button on the Mac, the navigation back on touch (pushed); it returns to the same scroll position.
- Context menu of a chart (box or page): Copy Image, Copy Values (the table as tab-separated text).
- The screen contract adds `stats.chartPage` with the unit that builds it (inventory rule: a new screen id lands with its code).

## S10. Loading, kept results and progress

Decided 2026-11-22 (Antoine: the Statistics page "is a bit of an exception … it's understood for users that this page can take a bit longer to generate"; it "needs to be performant", but reading hundreds of thousands of reviews cannot always be instant). This replaces round 11's "Loading: none visible" and its "no spinner, nothing moves" for this page only; every other screen keeps the instant rules.

### S10.1 Tiers

One refresh asks for every part at once (`stats/history-reads` H5, H15); the core returns them in three tiers and the page fills each box as its parts arrive:

1. the present state: the strip's Studied Today, Remaining Today and Mature, Card Counts, Reviews Due, Retrievability, Stability, Difficulty;
2. the review log: Reviews Over Time, Activity, Retention, Forgotten, Cards Over Time, Added and Started, Answers, Time of Day, By Deck;
3. knowledge: Knowledge Over Time, last.

The strip's Retention Today and Streak come with tier 2 (`stats/history-reads` H5: `retention`, `activity_days`); until then they show the small progress indicator in place of their value.

### S10.2 Kept results

- The page keeps the last result it showed for each (collection, page scope, period), in memory and on the device (the app's caches directory: never synced, never in the collection, deleted with no loss). A kept result survives a relaunch for the same collection and is dropped when the collection is replaced (`CollectionReplaced`) or another collection opens.
- When a kept result exists, the page shows it at once, every box filled, and refreshes in place: new numbers replace old ones without moving the layout, scroll position, selection and controls kept.
- A partial result is kept as partial: the boxes it had are shown, the others as in S10.3.

### S10.3 Honest loading when nothing is kept

On a first open, a deck or period never viewed, or after the collection was replaced: every box is drawn at its final size with its title and legend, and a box whose data is not ready shows the system's small progress indicator inside its plot area, determinate when the core reports a fraction for its part (H15: the log pass, the knowledge replay), indeterminate otherwise. Boxes fill in as each tier completes. No full-page spinner, no skeleton or shimmer, no overlay.

- With All and nothing kept, the page makes two refreshes in a row: first one with `last12Months`, whose boxes it draws in All's monthly bars, with the older months empty and the review-log boxes' indicators still showing (the axis already spans from the first month, H9); then one with `allHistory`, a new refresh with its own context (`stats/graphs-common` G8.3), whose parts replace every box in place as they arrive. Each refresh is one request.

### S10.4 The status in the subtitle

The toolbar subtitle (Mac) or the navigation subtitle (touch) reads "<n> cards in <m> decks" (S3.1), followed while a refresh runs over a shown result by " · Updating…". After a refresh completes it reads " · Updated just now" for one minute, then nothing. When the shown numbers are kept ones and the refresh could not finish (an error, or the page left), it reads " · Updated <time>", the time the shown result was read (the short time today, else the date). Never a blocking overlay.

### S10.5 Cancellation

Changing the deck, changing the period to or from All, leaving the page, or a newer refresh cancels the superseded request (H16); the boxes keep what they show until the new parts arrive. A period change among 1M to 1Y keeps the running request, which already asks for `last12Months`: its parts arrive and are drawn in the new period's bins. The page stays scrollable and its controls respond throughout (S19).

### S10.6 No precomputation required

Nothing is computed before the page asks: the history store builds on the first request that needs it (H14.2). Starting the reads earlier (for example once Today has been idle) is an allowed later optimisation, not a requirement.

## S11. Live refresh

- A visible page refreshes by itself, in place, half a second after the last of: a published change set, a review, an undo, a sync, a rollover or another day-clock change, `CollectionReplaced` (`stats/graphs-common` G10). It keeps its scroll position, selection and controls, and shows S10.4's status.
- A hidden page refreshes when it appears. Each open of the page refreshes (R1294).
- There is no Refresh command and no pull to refresh: the page refreshes after changes by itself.
- After `CollectionReplaced` the kept results are dropped (S10.2) and the page reads again as S10.3 says.

## S12. States

### S12.1 No collection open

While no collection is open (the welcome screen, a collection still opening, a failed open): the Statistics section shows what the shell shows for that state in every section, with no boxes; its toolbar controls are disabled. With no collection on the device, the system's empty view titled "No Collection" with "Statistics appear once a collection is open." Every screen unit checks this state and first run (S20).

### S12.2 An empty collection

No cards: one system empty view, "No Cards", with "Add notes or import a collection to see statistics.", instead of twenty empty boxes; the toolbar controls are disabled.

### S12.3 No reviews yet

Cards but no review (an imported deck before its first study): the charts of the present work at once (Card Counts, Reviews Due with its first day); each chart of the past shows S8.5's line, "No reviews yet", with "This fills in as you study." under Knowledge Over Time. Added and Started shows the cards added.

### S12.4 Sparse

A few days of study: bars keep their fixed width, a line of one point is a point, a rate from fewer than 20 reviews is an open point (S7.2), and Time of Day shows bars without points (S8.5).

### S12.5 A deck filter

Every figure follows the deck, the strip included. The Decks section lists its subdecks (S7.15). A deck with no data shows S8.5's lines with the deck's name.

### S12.6 A filtered deck

The page shows the cards the filtered deck holds now, with their whole review history (G2.1), and one line under the strip: "A filtered deck shows the cards it holds now. Their reviews also count in their home decks." By Deck shows "This deck has no subdecks" unless it has some.

### S12.7 A deleted deck

A deck deleted while it is the filter (`unknown_deck`, G8.2), or kept from an earlier visit (S3.3): "This deck no longer exists." in place of the content, with a Show All Decks button that sets All Decks.

### S12.8 Errors

- An unreadable row (`unreadable_row`, G4) in the scope: a content-unavailable view in place of the sections, "Statistics Unavailable", with "A review or card record can't be read. Check Database can repair it." and Check Database once U128 lands. The strip and controls stay.
- A part that fails alone (H15) shows its box's line "Couldn't load this chart." and the other boxes stay.

## S13. Ways in

| From | Opens |
|---|---|
| The sidebar's Statistics row (Mac, iPad), second under Today, with `chart.bar.xaxis`; View › Statistics ⇧⌘S | the page with the kept deck and period (S3.3) |
| The Statistics tab (iPhone, the folded Duo: between Decks and Browse; the Duo opened: the side tab bar's second item) | the same |
| Deck Statistics in a deck's menu (the row's context menu, the menu bar's Deck menu, the touch More menu), `chart.bar.xaxis` | the page on that deck (S3.3), normal and filtered decks |
| A deck page's Activity header | the page on that deck |
| T on Today and on a deck page (Mac, iPad with a keyboard; R1349) | the page; on a deck page, on that deck |
| A row of By Deck | the same page on that deck |

- Deck Statistics has no key of its own. ⇧⌘S is free: Meguri has no documents, so no Save As.
- The section stays hidden (`availableSections`) until the units of S20's last line land, as Browse was.

## S14. Keyboard

- Tab moves through the toolbar's deck pop-up and period control, then box by box (each box's header button, then its switches, then its plot).
- In a focused plot, Left and Right move the selection one bar at a time; Return runs Show in Browse when the bar offers it; Esc clears the selection.
- ⌘[ returns from a chart's page. View › Go to Section's items have no keys.

## S15. Accessibility

`stats/graph-presentation` P18 stays, with these changes:

- Each chart is one element whose label is its figure's sentence (for example "Estimated knowledge, 3,512 of 3,950 studied cards, up 412 in a year."), with an Audio Graph (`AXChartDescriptor`: one category per bar, named by its period; one series per stacked kind or line) and one element per bar inside it, in runs of ten (P18.2), each reading the callout's text. Show in Browse is a named action on the elements that offer it.
- Series are told apart by legend, order and name as well as colour, and follow Increase Contrast.
- Text follows Dynamic Type to the largest accessibility size; at the accessibility sizes legends and figures stack under the plot and the plot keeps its height.
- The strip's figures, the tables' rows and the Activity grid's days are elements (S6, S7.5: a month container per year block, as `stats/heatmap-presentation` M15 does).
- Identifiers: `stats.<element>` (R1445): `stats.deck`, `stats.period`, `stats.strip.<figure>`, `stats.section.<section>`, `stats.box.<box>`, `stats.box.<box>.header`, `stats.chartPage.<box>`, `stats.goToSection`.

## S16. Strings

Meguri's own, in the String Catalog; `<…>` marks a value. Anki's strings that stay are `stats/graph-presentation` P19's retention table and its help text (S7.2) and the memory-state footnote.

| Where | Text |
|---|---|
| Section, tab, menu | `Statistics`, `Deck Statistics`, `Go to Section` |
| Controls | `All Decks`, `1M`, `3M`, `6M`, `1Y`, `All`; `By Type`, `By Answer`, `Count`, `Time`, `All`, `Young`, `Mature` |
| Subtitle | `<n> cards in <m> decks`, `<n> cards in 1 deck`, `Updating…`, `Updated just now`, `Updated <time>` |
| Strip | `Studied Today`, `Retention Today`, `Remaining Today`, `Streak`, `Mature`, `None yet`, `None`, `<n> tomorrow`, `<n> days`, `of <n>` |
| Sections | S5's titles and questions |
| Box titles | `Knowledge Over Time`, `Retention`, `Forgotten`, `Reviews Over Time`, `Activity`, `Cards Over Time`, `Card Counts`, `Added and Started`, `Reviews Due`, `Answers`, `Time of Day`, `Retrievability`, `Stability`, `Difficulty`, `By Deck` |
| Series | `Known`, `Studied`, `Desired retention`, `Average desired retention`, `Remembered`, `Forgotten`, `New`, `Learning`, `Relearning`, `Young`, `Mature`, `Suspended`, `Buried`, `Filtered`, `Again`, `Hard`, `Good`, `Easy`, `Added`, `Started`, `Overdue`, `Total by then`, `Reviews` |
| Markers | `Estimated`, `Now` |
| Figures | S7's figure labels and sentences |
| Empty and states | `No reviews yet`, `This fills in as you study.`, `No reviews in the last month`, `… the last 3 months`, `… the last 6 months`, `… the last year`, `No reviews in <deck> in <span>`, `No cards due`, `No cards with a memory state`, `No cards with a memory estimate`, `Later`, `No Collection`, `Statistics appear once a collection is open.`, `No Cards`, `Add notes or import a collection to see statistics.`, `This deck no longer exists.`, `Show All Decks`, `This deck has no subdecks`, `A filtered deck shows the cards it holds now. Their reviews also count in their home decks.`, `Statistics Unavailable`, `A review or card record can't be read. Check Database can repair it.`, `Couldn't load this chart.`, `<n> suspended cards are not shown.`, `<n> suspended cards are not included.`, `<n> cards could not be estimated.`, `An open point has too few reviews to trust` |
| Actions | `Show in Browse`, `Cards Reviewed on <date>`, `Cards Forgotten on <date>`, `About This Chart`, `Copy Image`, `Copy Values` |

`Show in Browse` replaces P19's `Show in Browser`, as the section is named Browse.

## S17. Writes nothing

Viewing statistics writes nothing to the collection (R1295, `stats/graphs-common` G11, `stats/history-reads` H17): no row, `usn`, `mod`, `col.mod` or config key; the three keys of S3.4 included. Choosing a deck never sets `curDeck` (R1353). The page's own state (S3.3) and kept results (S10.2) are device-local files.

## S18. What this replaces

- In `stats/graph-presentation`: P5 (per-graph ranges, their memory and the history following), P9's donut and the Separate toggle, P6's Backlog toggle and running total, P8's ranges and table text, P10's Intervals box and Stability's quantile ranges, P11's quantile ranges, P13's two-axis chart, P14's twelve bars, P15's ranges, P16.5's "No Data", P17.1, P17.2, P19's titles and P2 and P3 for this page (P24 lists each).
- In `stats/graphs-common`: G3's default (the page's first period is All), G7's writes (the page writes none), G8.3's two requests per refresh (one request, parts in tiers, `stats/history-reads` H15).
- In `stats/studied-today`: T3 and T4's Today section on the Stats screen (the strip replaces it; T12 is unchanged).
- In `stats/pdf-export`: not in the first version (9A).
- In `docs/design/screen-inventory.yaml`: the earlier text of `stats.overview` (the scope picker, the history picker, the thirteen boxes in Anki's order, the heatmap and streaks "not here").
- Plan requirements, each with its note in the parent plan: R1350 (one deck pop-up; no Collection or Search scope in this version), R1352 (default All), R1354 (kept deck and period), R1355 (one request per refresh, parts in tiers), R1838 in part (every part is asked for at once, not only those scrolled into view; its 300 ms holds), R1357 to R1359 (sections and the parts of a box), R1361 (S8.5), R1362 (the page's own bins, not Anki's), R1364, R1366, R1367 (one period), R1369 to R1372 (the strip), R1375, R1376, R1377 (Reviews Due), R1379 to R1383 (Activity), R1386 to R1389 (Reviews Over Time), R1391, R1392 (Card Counts), R1394 to R1396 (no Review Intervals box), R1399 to R1401 (Stability and Difficulty bins and colours), R1410, R1412 (Time of Day), R1414 (Answers), R1415, R1416 (Added and Started), R1421 (later, 9A). R1363's colour list changes as S8.1 says.

## S19. Speed

Targets to measure, not gates that shape the design (decided 2026-11-22): S10's tiers, kept results and progress keep a slow read visible and honest. On the reference devices (R1802), Release build, the 150k fixture:

| What | Target | How measured |
|---|---|---|
| The page with a kept result | every box filled in the first frame after navigation; usable at once | `perf-ui` `StatsPerfTests`, interval `stats.keptOpen` |
| The present tier (S10.1 1) | well under 100 ms from the request | `stats.presentTier`; core `stats_present` (H19) |
| The review-log tier, caches current, 1M to 1Y | under 500 ms (R1837's all graphs) | `stats.logTier` |
| A deck or period change, caches current | under 300 ms to the last tier-2 box (R1838) | `stats.scopeChange` |
| A cold pass (nothing kept, caches not built, 1.2 million rows) | a few seconds on an iPhone, determinate progress shown, every box filling as it completes | `stats.coldOpen`, device run |
| Scrolling and the controls during any read | no hitch above 5 ms/s (as R1829); a control responds in one frame | `stats.scrollDuringRead` |
| Selection, switches, period changes within the year | one frame, no core call (S3.2) | `stats.selection` |
| Show in Browse | Browse's first page within its own budget (`browser/workspace` V17) plus H13's 50 ms | `stats.showInBrowse` |

The rows' Product Contract ids are the plan owner's to assign (the plan's §3).

## S20. Tests

- **Model tests** (`MeguriKit`, `swift-test`): S4's bins for each period across a week start of Sunday and of Monday, a month boundary, a year boundary and a leap day; Reviews Due's ahead bins and the Overdue bar; S4.3's windows picked from the records; the streak rules (S7.5): today not yet studied, a gap, the longest run across a year; S3.3's kept state (first time All, opening from a deck, an invalid stored switch); S7.13's and S7.14's bins with their edge keys; S7.2's open points at 19 and 20 reviews; S10's tier filling and the All-first-year sequence; S10.4's subtitle texts; S11's half-second coalescing.
- **By construction**: the last bar of Cards Over Time equals Card Counts' `including_inactive` minus `suspended_by_state` on the same result; Knowledge Over Time's Known value is at most its Studied value at every bar; Studied Today equals the Today page's for All Decks; By Deck's rows add up (H20).
- **UI** (macOS test VM, one job at a time; simulators): for every screen unit, the first-run state (no collection on the device) and S12.1 to S12.7, light and dark, the largest text size; the Mac page at 560, 1,250 and 1,600 points; the iPhone column; the Duo opened (nothing on the fold); Show in Browse and Back keeping the scroll position.
- **Accessibility** (`tools/mg a11y`): each chart's label, its audio graph and per-bar elements; the strip's elements; Differentiate Without Colour's separating lines.
- **Performance**: S19's intervals in `StatsPerfTests` and the core benchmarks of H19.
- The Statistics section is shown (S13's last line) once U296 to U302 and U304 to U306 have landed (U307); Show in Browse (U303) may come after, and until then no callout offers it.

## S21. Later

Not in this version (round 11, 9A and the page's Later list): a search scope (Browse's "Statistics for These Cards"), File › Export Statistics as PDF… and the touch share button (`stats/pdf-export`, to be rewritten for the sections), a simulated workload, goals, retention per learning step, future reviews on the year grid, a page the user arranges.
