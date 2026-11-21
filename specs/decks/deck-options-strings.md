# Deck options strings

Spec: decks/deck-options-strings
Requirements: R731, R736, R739, R740, R741, R742, R743, R745, R746, R747, R748, R749, R750, R751, R752
Anki: 26.09.3 (the English of Anki desktop's deck options screen, except the differences K8 lists)
Scenarios: none (screen text; U232 to U234 test it, K9)

The deck options screen text that must be Anki's, because users look these names up in Anki's manual and forums: the section and option names (R736), the limit tabs (R740, R742), the labels of every choice list (R743, R745, R746, R750) and the preset picker's label (R731). U232, U233 and U234 take these texts only from this file. Every other text on the screen is Meguri's (K7). The options, their fields, values and order are `sched/deck-options` O3; this file adds only their display text. Where O3, O11 or a screen contract gives a name or a choice in short form (O3's `0 Sequential`, or "ascending difficulty" for value 6 of Review sort order), the text shown is the one here (`Sequential (oldest cards first)`, `Easy cards first`), and the names and choice labels here are Anki's, not Meguri's.

## K1. Keys and form

- Every string is an entry of the Deck Options String Catalog with the stable key given here. The English value is the one given; other languages are Meguri's own translations of the English. Keys never change once shipped; a changed meaning gets a new key.
- `%@` is a text argument and `%lld` an integer argument; with several arguments they are numbered (`%1$@`, `%2$lld`). A plural entry varies on its integer argument: English uses **one** for exactly 1 and **other** for every other value, 0 included.
- Text is shown exactly as given: sentence case for option names and choice labels, title case for section names and some choices, as Anki writes them. Meguri does not change their case to match other controls.
- Apostrophes are typographic (’), as `review/reviewer-strings` S1 decides for the reviewer; Anki writes ASCII `'` (K8).
- The same English word in two lists (`Random`, `Show Reminder`) has one key per list, so a translation can differ by context.

## K2. Sections (R736)

In the order the screen shows them under the Advanced disclosure (`sched/deck-options` O3).

| Key | English |
|---|---|
| `deckOptions.section.dailyLimits` | Daily Limits |
| `deckOptions.section.newCards` | New Cards |
| `deckOptions.section.lapses` | Lapses |
| `deckOptions.section.displayOrder` | Display Order |
| `deckOptions.section.burying` | Burying |
| `deckOptions.section.audio` | Audio |
| `deckOptions.section.timers` | Timers |
| `deckOptions.section.autoAdvance` | Auto Advance |
| `deckOptions.section.advanced` | Advanced |

- The Essentials section and the Advanced disclosure that holds these sections are Meguri's (K7). The disclosure's label is its own key, even where its English is also `Advanced`.
- No FSRS section is shown, so it has no title here: O3's FSRS rows are Desired retention and its This deck tab, which show only in Essentials (R736), and the rest of Anki's FSRS section is hidden (R752).

## K3. Option names (R736, R739 to R751)

One name per option of `sched/deck-options` O3, in O3's order. An option shown in Essentials keeps the same key and name there (R736: each option appears once).

| Key | English | Field or key (O3) |
|---|---|---|
| `deckOptions.option.newPerDay` | New cards/day | `new_per_day` |
| `deckOptions.option.reviewsPerDay` | Maximum reviews/day | `reviews_per_day` |
| `deckOptions.option.newCardsIgnoreReviewLimit` | New cards ignore review limit | `newCardsIgnoreReviewLimit` |
| `deckOptions.option.applyAllParentLimits` | Limits start from top | `applyAllParentLimits` |
| `deckOptions.option.desiredRetention` | Desired retention | `desired_retention` |
| `deckOptions.option.learnSteps` | Learning steps | `learn_steps` |
| `deckOptions.option.insertOrder` | Insertion order | `new_card_insert_order` |
| `deckOptions.option.relearnSteps` | Relearning steps | `relearn_steps` |
| `deckOptions.option.minimumLapseInterval` | Minimum interval | `minimum_lapse_interval` |
| `deckOptions.option.leechThreshold` | Leech threshold | `leech_threshold` |
| `deckOptions.option.leechAction` | Leech action | `leech_action` |
| `deckOptions.option.newGatherPriority` | New card gather order | `new_card_gather_priority` |
| `deckOptions.option.newSortOrder` | New card sort order | `new_card_sort_order` |
| `deckOptions.option.newMix` | New/review order | `new_mix` |
| `deckOptions.option.interdayLearningMix` | Interday learning/review order | `interday_learning_mix` |
| `deckOptions.option.reviewOrder` | Review sort order | `review_order` |
| `deckOptions.option.buryNew` | Bury new siblings | `bury_new` |
| `deckOptions.option.buryReviews` | Bury review siblings | `bury_reviews` |
| `deckOptions.option.buryInterdayLearning` | Bury interday learning siblings | `bury_interday_learning` |
| `deckOptions.option.disableAutoplay` | Don’t play audio automatically | `disable_autoplay` |
| `deckOptions.option.skipQuestionWhenReplayingAnswer` | Skip question when replaying answer | `skip_question_when_replaying_answer` |
| `deckOptions.option.capAnswerTime` | Maximum answer seconds | `cap_answer_time_to_secs` |
| `deckOptions.option.showTimer` | withdrawn 2026-11-19 (`review/timers#T2`): not in the catalog, not shown, the key not reused | `show_timer` |
| `deckOptions.option.stopTimerOnAnswer` | withdrawn 2026-11-19 (`review/timers#T2`): not in the catalog, not shown, the key not reused | `stop_timer_on_answer` |
| `deckOptions.option.secondsToShowQuestion` | Seconds to show question for | `seconds_to_show_question` |
| `deckOptions.option.secondsToShowAnswer` | Seconds to show answer for | `seconds_to_show_answer` |
| `deckOptions.option.waitForAudio` | Wait for audio | `wait_for_audio` |
| `deckOptions.option.questionAction` | Question action | `question_action` |
| `deckOptions.option.answerAction` | Answer action | `answer_action` |
| `deckOptions.option.maximumInterval` | Maximum interval | `maximum_review_interval` |
| `deckOptions.option.historicalRetention` | Historical retention | `historical_retention` |
| `deckOptions.option.ignoreRevlogsBefore` | Ignore cards reviewed before | `ignore_revlogs_before_date` |

- The Load balancer toggle (`loadBalancerEnabled`) has no Anki name: Anki desktop shows no such option (`sched/deck-options` O12). Its label is Meguri's (K7).

## K4. Limit tabs (R740, R742)

| Key | English | Shown on |
|---|---|---|
| `deckOptions.tab.preset` | Preset | New cards/day, Maximum reviews/day, Desired retention |
| `deckOptions.tab.thisDeck` | This deck | the same three |
| `deckOptions.tab.todayOnly` | Today only | New cards/day and Maximum reviews/day only |

## K5. Choice labels (R743, R745, R746, R750)

Each list in the menu order of `sched/deck-options` O3, as value → label.

| Key | Value | English |
|---|---|---|
| `deckOptions.insertOrder.sequential` | 0 | Sequential (oldest cards first) |
| `deckOptions.insertOrder.random` | 1 | Random |
| `deckOptions.leechAction.suspend` | 0 | Suspend Card |
| `deckOptions.leechAction.tagOnly` | 1 | Tag Only |
| `deckOptions.gather.deck` | 0 | Deck |
| `deckOptions.gather.deckThenRandomNotes` | 5 | Deck, then random notes |
| `deckOptions.gather.ascendingPosition` | 1 | Ascending position |
| `deckOptions.gather.descendingPosition` | 2 | Descending position |
| `deckOptions.gather.randomNotes` | 3 | Random notes |
| `deckOptions.gather.randomCards` | 4 | Random cards |
| `deckOptions.newSort.cardTypeThenGathered` | 0 | Card type, then order gathered |
| `deckOptions.newSort.gathered` | 1 | Order gathered |
| `deckOptions.newSort.cardTypeThenRandom` | 2 | Card type, then random |
| `deckOptions.newSort.randomNoteThenCardType` | 3 | Random note, then card type |
| `deckOptions.newSort.random` | 4 | Random |
| `deckOptions.mix.mixWithReviews` | 0 | Mix with reviews |
| `deckOptions.mix.afterReviews` | 1 | Show after reviews |
| `deckOptions.mix.beforeReviews` | 2 | Show before reviews |
| `deckOptions.reviewSort.dueThenRandom` | 0 | Due date, then random |
| `deckOptions.reviewSort.dueThenDeck` | 1 | Due date, then deck |
| `deckOptions.reviewSort.deckThenDue` | 2 | Deck, then due date |
| `deckOptions.reviewSort.ascendingIntervals` | 3 | Ascending intervals |
| `deckOptions.reviewSort.descendingIntervals` | 4 | Descending intervals |
| `deckOptions.reviewSort.easyFirst` | 6 | Easy cards first |
| `deckOptions.reviewSort.difficultFirst` | 5 | Difficult cards first |
| `deckOptions.reviewSort.ascendingRetrievability` | 7 | Ascending retrievability |
| `deckOptions.reviewSort.descendingRetrievability` | 11 | Descending retrievability |
| `deckOptions.reviewSort.relativeOverdueness` | 12 | Relative overdueness |
| `deckOptions.reviewSort.random` | 8 | Random |
| `deckOptions.reviewSort.added` | 9 | Order added |
| `deckOptions.reviewSort.latestAddedFirst` | 10 | Latest added first |
| `deckOptions.questionAction.showAnswer` | 0 | Show Answer |
| `deckOptions.questionAction.showReminder` | 1 | Show Reminder |
| `deckOptions.answerAction.buryCard` | 0 | Bury Card |
| `deckOptions.answerAction.answerAgain` | 1 | Answer Again |
| `deckOptions.answerAction.answerGood` | 2 | Answer Good |
| `deckOptions.answerAction.answerHard` | 3 | Answer Hard |
| `deckOptions.answerAction.showReminder` | 4 | Show Reminder |

- The `mix` keys serve both New/review order and Interday learning/review order, which have the same list.
- Review sort order shows Anki's FSRS labels: value 6 (stored as descending ease) is `Easy cards first` and value 5 (ascending ease) is `Difficult cards first`, in that order, as O3 lists them. Anki's ease labels (`Ascending ease`, `Descending ease`) are never shown (R746, R752).

## K6. Preset picker (R731)

| Key | English (one / other) | Arguments |
|---|---|---|
| `deckOptions.preset.usedBy` | `%1$@ (used by %3$@ deck)` / `%1$@ (used by %3$@ decks)` | `%1$@` the preset's name as the draft holds it; `%2$lld` the use count the picker shows (`sched/deck-options` O6.1, O6.3), which chooses the plural form and is not shown; `%3$@` that count written in the locale's integer format with grouping |

- Examples: `Languages (used by 4 decks)`, `Default (used by 1 deck)`, and `Test (used by 0 decks)` for a preset the draft has just added (`sched/deck-options` O6.3).
- The shown count is `%3$@`, never `%2$lld`: a printf-style integer does not group, and Anki's translated text writes the number grouped (`Languages (used by 1,234 decks)`). This is the convention of `stats/heatmap-presentation` M10.3.
- The name is shown as stored, never trimmed or escaped.

## K7. Meguri's texts (not listed here)

These are Meguri's own (R734, R1959, `sched/deck-options` O11) and take their wording from Meguri's String Catalog, not from Anki: the screen's title, the Essentials section, the Advanced disclosure, the "Global" badge (R735), every help popover (R734, R747), the restore-default control, the warnings and errors of `sched/deck-options` O4 (R739 "Anki's warning" is the condition, not the wording), the Discard / Keep editing prompt, the preset menu items and name prompt (R732), the removal confirmation (R733), the "Waiting for" banner and the reload banner (U232), the Load balancer label (R751), `Off` for a zero Auto Advance time (U234), the percentage form of desired retention (R742), and the ignore-date count line (`sched/deck-options` O2 item 7). The save's undo step name, `Deck Options`, is `sched/deck-options` O7's (`conformance/undo-ops` V4).

## K8. Differences from Anki

- Apostrophes are typographic (K1); Anki writes `Don't play audio automatically`.
- Anki's English for Question action ends with a space; Meguri's has none.
- Review sort order never offers the two ease labels (K5).

## K9. Tests

No scenario: these texts are not core output. U232's String Catalog test compares the English value of every key in K2 to K6 with these tables, and U234's test of each picker compares its labels and their order with K5.

## K10. The first version's page (Meguri, 2026-11-20)

The page of `sched/deck-options` O17 (`docs/design/native-interface-contract.md` N20.9). Decided 2026-11-20 (option 6B): its row labels use Apple's title case over Anki's wording, so for these three options K3's English is not shown on this page; K3 stays for the Advanced page. These texts are Meguri's.

| Key | English | Shown |
|---|---|---|
| `deckOptions.v1.title` | Deck Options | the page's title |
| `deckOptions.v1.preset` | Preset | the preset row's label |
| `deckOptions.v1.presetFooter` | `Used by %2$@ decks. Changes apply to all of them.` / one: `Used by this deck only.` | the preset row's footer; `%1$lld` the use count (O6.1), which chooses the form and is not shown; `%2$@` that count in the locale's integer format with grouping (K6's convention) |
| `deckOptions.v1.section.dailyLimits` | Daily Limits | section header |
| `deckOptions.v1.newPerDay` | New Cards per Day | row |
| `deckOptions.v1.reviewsPerDay` | Maximum Reviews per Day | row |
| `deckOptions.v1.section.memory` | Memory | section header |
| `deckOptions.v1.desiredRetention` | Desired Retention | row |
| `deckOptions.v1.desiredRetentionFooter` | How likely you are to remember a card when it comes back. | Memory's footer |
| `deckOptions.v1.ownLimits` | This deck has its own limits, which stay in force. | Daily Limits' footer, when the deck holds its own limit (O17.1) |
| `deckOptions.v1.ownRetention` | This deck has its own desired retention, which stays in force. | added to Memory's footer, when the deck holds its own desired retention |
| `deckOptions.v1.save` | Save | the confirmation button |

- The preset menu's items keep K6's `deckOptions.preset.usedBy`.
- The Discard prompt uses the editor's "Discard changes?", Discard and Keep Editing (`editor/editing-strings`).
