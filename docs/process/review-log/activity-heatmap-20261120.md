# Activity heatmap (N14.11, `stats/activity-heatmap`)

Starting commit `c2b24f49c4a70aa36badc6deb6cdc9f79987152b`. Sessions:
`s-20261120T072705Z-efe18a55` (spec writer), `s-20261120T072711Z-806de9cf`
(implementer), and `s-20261120T111041Z-305c9d30` (fixer, which finished the
work after the implementer session was interrupted). Pull request #10.

## Reviews

1. Implementer-session reviews, before the interruption. They produced:
   - `83bbf097d`: row widths kept above the views, and a retry after a failed read;
   - `574d6e1fc`: rows on screen are counted, and each list keeps its own width;
   - `a08bf6514`: header contrast, from the accessibility audit;
   - `4c0b7d6c2`: a flat FFI record, after a similarity flag on the struct channel.
2. Independent read-only review of the final diff, by a fresh reviewer with no
   Anki sources. It found no blockers and nothing that looks copied. It found
   three should-fix items and one test gap, all fixed in `825c1dfd9`:
   - two rows of a page could overlap and resize each other;
   - a scope known to be empty could flash its header;
   - a read refused while a long op held the collection waited for the next
     lifecycle change;
   - the plan test allowed a review-log search without the rowid bound.

   It also raised these nits, left as they are:
   - a rollover can cost one or two extra queries;
   - the counts have no thousands separator, because the plural form takes an integer;
   - level 1 and an empty day are close in contrast.

   An owner question, which is on the pull request: a filtered deck's page shows no heatmap.
3. The foldable-kind capture of `825c1dfd9` showed squares overflowing the
   row's margins: they set the width that was then read back. Fixed in
   `afb903a64`: the squares now sit in the overlay of the view that measures.
4. The hosted AI reviewer's comments on the pull request were fixed in
   `afb903a64` and `b46edf319`:
   - a readiness signal during a failing read;
   - a hidden row kept an old width;
   - a nextest slot for the bounded million-row test;
   - the model's seeded random source in the fixture.

   The remaining comment, on cancellation before the core read, says the code is correct.

## Evidence

The focused checks, the captures and the query plans and times are listed in
the pull request body.
