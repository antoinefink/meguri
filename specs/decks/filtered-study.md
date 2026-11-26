# Filtered decks and Custom Study

Spec: decks/filtered-study
Requirements: R53, R54, R436, R612, R628, R706, R707, R1459, R1483
Anki: 26.09.3
Scenarios: conformance/scenarios/decks/delete/, conformance/scenarios/sched/queue/gather/

The feature request of 2026-11-26 supersedes the first release's filtered-study exclusions. This contract governs a filtered deck selected directly. A normal review and Today's walk continue to exclude cards temporarily held elsewhere.

## F1. Definition

A filtered deck owns a search definition. Its members are existing collection cards on temporary assignment. Editing, syncing or opening a collection preserves the deck identity, its protobuf fields including unknown data, and the cards' home identities. Use the normal deck tree's name and parent validation. A filtered deck cannot be a parent. A name collision must never turn a normal deck into a filtered deck.

The editor accepts a name, a search in the Browse grammar, a card limit and order, and an optional second independent search with its own limit and order. The first term defaults to 100 random cards. Opening from a normal deck pre-fills a literal deck search for that deck and its descendants with `is:due`. The second term starts disabled, with a suggested limit of 20 in due order and `is:new` in the same scope. Opening from Browse uses the active card search. An empty search matches any eligible card. Rescheduling defaults on. When off, the editor exposes the three F4 delays. The representation and order codes are in `model/protobuf-records#P6.5.3` and P6.5.4. Opening or cancelling the form makes no collection changes.

## F2. Build, rebuild, empty and delete

Create-and-build and edit-and-build validate every enabled term and perform one undoable collection operation. Rebuild first releases the target's current members, so a card that still matches can be gathered again. Run each term in order, taking at most its limit of eligible matches after sorting. Exclude suspended and buried cards and every card already in a filtered deck, including cards taken by the first term. Learning and relearning cards are eligible. Ignore normal study limits when gathering. Treat the user's whole expression as one grouped operand when intersecting eligibility. A second term with a syntax error rolls back the whole operation.

| Order code | Selection order before applying the limit |
|---|---|
| 0 | Oldest last review first, with cards lacking a review log first |
| 1 | A new random order for each build |
| 2, 3 | Interval ascending, descending |
| 4 | Most lapses first |
| 5, 7 | Oldest and newest note first respectively, then ascending card ordinal within each note |
| 6 | Due time ascending, then card ordinal. Compare a day due as today's current time plus the difference in days times 86,400. Intraday timestamps above 1,000,000,000 already use seconds. |
| 8, 9 | FSRS retrievability ascending, descending respectively, as in `sched/orders#O4` |
| 10 | Relative overdueness as in `sched/orders#O4` |

Use the signed tie-break hash of card id and modification seconds defined in `sched/orders#O1`. Assign gathered cards their filtered deck id, with their home deck and prior due saved in `odid` and `odue`. Positive dues become successive build positions beginning at −100,000, across both terms. Nonpositive dues retain their values. In rescheduling mode preserve the queue. In preview mode initially use review queue 2. Card type, scheduling history and note identity survive the move. Stamp changed rows for sync.

Create or edit with no matches reports that no eligible cards matched and preserves the previous definition and members, unless the person explicitly allows an empty result. A standalone Rebuild may successfully leave a deck empty. Empty returns its cards per `decks/decks#D9` and keeps the saved definition. Delete removes the deck and returns the cards by the same rule. Each command is one undoable operation. Neither creates or deletes a note or a study card. Collection reload and sync must preserve a partially completed deck.

## F3. Study with rescheduling

Study the selected filtered deck by its stored membership. Imported and locally built decks use the same queue. Within each queue, the build positions give the due order. Keep intraday priority, learn-ahead and mix rules. New siblings retain their gathered order.

For scheduling, read the card type, home preset, memory state and home deck's desired retention. Read the original due when it is nonzero, otherwise read the current due. For an early review, the due day in `sched/fsrs-transitions#T3` is the effective due, even when it exceeds today. T2 supplies the elapsed days since the last review to FSRS. The build position is an ordering value. It cannot replace the home due in this calculation (R702).

Apply the selected normal transition. When the result is review, return the card to its home deck, clear both original fields and save the newly computed schedule. When the result is learning or relearning, retain temporary membership and save the next due in both due fields, so Empty will retain that progress. Normal answer effects apply, including review history, sibling handling, undo and the counters for the deck in which the answer was given (`sched/presets#D4`). An early review writes review-log type 3 with its scheduling factor. A review on time or late writes type 1. Learning and relearning use their own normal log types.

## F4. Preview without rescheduling

Again, Hard and Good use the deck's configured delays in seconds. New definitions default to 60, 600 and 0 respectively. Easy always finishes. A zero delay finishes the card for any answer. A positive delay leaves the original fields in place and uses preview-repeat queue 4 with an intraday due based on the scheduler's ordinary fuzzed learning timestamp. Queue 4 follows the intraday cutoff and learn-ahead rules after a reopen as well. A finishing answer returns the card home under D9. All four answers write a type-3 review log with factor zero. Preserve the card's interval, ease, learning steps, repetition and lapse counts, FSRS memory and last-review timestamp. Undo restores the assignment, queue and review log.

A home review due on collection day zero has a genuine zero in the saved due field. F2 preserves the initial zero position. After a timed preview repeat, however, the current due holds a timestamp. The pinned reference treats the saved zero as absent and leaves the timestamp as a review day when it returns home. Meguri's narrow schedule-preservation rule is to restore day zero when all of these hold: a preview deck, valid home membership, review type, preview-repeat queue 4 and saved due zero. This case is observable from the existing fields on reopen or sync. Apply it equally to a finishing preview answer, Empty, Rebuild's release and Delete. All other combinations keep D9's existing restoration rule. Verify both that original day zero survives Again → reopen → Easy (and Again → Empty or Delete), and that a nonzero saved day is restored verbatim. No auxiliary storage field is needed.

For example, preview a review card from deck A with interval 21 and due day 103. After Again it remains in the filtered deck with saved home A and saved day 103. After Easy, it belongs to A in the review queue with due day 103 and interval 21. Empty between the two answers has the same scheduling result, with only the first review-log entry.

## F5. Custom Study

Start from a normal deck. Scope every search to the selected deck and descendants using a literal deck-name operand. Preserve actual names containing quotes, wildcards, separators or search operators. A successful saved study deck becomes the selected deck. Reuse the dedicated filtered deck named “Custom Study Session”. If a normal deck has that name, explain the conflict and leave it untouched. If an existing custom session would have no matches, preserve its previous definition and cards.

| Choice | Effect |
|---|---|
| Adjust today's new limit | Subtract the signed delta from today's new studied counter. |
| Adjust today's review limit | Subtract the signed delta from today's review studied counter. |
| Forgotten cards | Preview in random order. Select the cards with an Again answer in the last N days (`rated:N:1`). |
| Review ahead | Study cards due by day today + N (`prop:due<=N`) in due order with rescheduling. |
| Preview recent new cards | Both `is:new` and `added:N`, oldest note first, preview. |
| Study by state or tags | Select a card limit and one of four modes below. Include any selected inclusion tag and exclude every selected exclusion tag, using literal tag operands and the normal hierarchical tag search. With no inclusions all otherwise eligible cards qualify. |

For the first two choices, reset stale daily counters to today before applying the delta. Apply the same delta to existing parents only when `applyAllParentLimits` is enabled. Never change child counters or the shared preset. Keep the positive delta as the selected deck's next suggested extension (`extend_new` or `extend_review`). Negative deltas reduce today's allowance. Display available uncapped new or review cards for the deck and its children. Keep the current deck selected and refresh its counts. Rollover expires the adjustment normally.

For day-based searches use a ceiling of 99,999 cards, with N initially 1. The forgotten-days range is 1–30. For state/tag study the initial limit is 100. New cards use `is:new` in oldest-note order with rescheduling. Due cards use `is:due` in due order with rescheduling. Previously introduced cards use `-is:new` in random order with rescheduling. All cards use random order and preview. On successful state/tag study save that source deck's included and excluded tag lists in its auxiliary config (`customStudyIncludeTags`, `customStudyExcludeTags`). A failed or cancelled operation leaves the saved choices untouched.

## F6. Native entry points and verification

Create Filtered Deck is available from deck navigation and from Browse with the current search. A filtered deck opens its own deck page from the list, sidebar and search. Study is the primary action when cards are available. Empty, Rebuild and Edit Filtered Deck operate on the selected filtered deck. Retain Browse Cards, Statistics, Rename and Delete. Deck Options for a normal deck still edits its preset. The filtered deck's settings edit the search definition. A normal deck page and its finished study state offer Custom Study, including when a daily limit is exhausted. The empty filtered deck offers Rebuild.

Use native controls and sheet conventions on each platform. Keep invalid input and failed builds visible in their originating forms, prevent duplicate submission, retain the draft on failure, and publish a successful result only into its still-current collection and route. A preview setting states that the home schedule will be preserved. Show only controls relevant to the selected custom study option.

Verify using synthetic cards in two normal decks and a filtered deck. Exercise overlapping terms and a suspended match, rebuild after a card changes, Empty and Undo, an early scheduled review, a preview repeat followed by a return home, a daily extension across rollover, and tag selection containing search punctuation. From a finished regular deck, build a custom session, study an actual card, then inspect its home schedule. Capture the native editor and Custom Study form from this fixture.
