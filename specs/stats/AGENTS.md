# specs/stats/

- Two day rules, never derived from each other: Anki's day (`day-buckets` B2, B3: fixed 86,400 s spans back from `next_day_at`) for the Stats screen, studied today and searches; the heatmap day (B4: local date of the instant moved back by the rollover hours, offset at the moved instant) for heatmaps, streaks and the widget.
- Heatmap rules follow the Review Heatmap add-on 1.0.1 (oracle: the add-on image), not Anki. The only deviations are R1280 (future rows on today), R1284 (deck pages by home deck) and B4.6 (the add-on's calendar draws a DST-start date's count on the day before; Meguri does not). Home's `limdecks` reads the current deck.
- Day-0 edges differ on purpose: studied today counts `id > S`, the Stats Today section `id >= S`, graph day indices truncate toward zero.
- Two number writers, never shared (`studied-today` T5): the Home line rounds binary32 values and never groups digits (Anki's core; the oracle compares its text); every Stats-screen line rounds the shortest decimal of a binary64 value and groups with `,` (Anki's web page; no oracle text, tested from spec tables).
- Stats-screen deck scopes match `did` or `odid` in the subtree (`deck:` semantics); deck-page heatmaps use the home deck only.
- No stats scenario, golden, deviation entry, schema shape or driver kind is in the tree yet (`conformance/stats-ops` H4, H8). B7, N9 and T9 list the scenarios that land with them.
