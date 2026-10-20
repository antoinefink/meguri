# Deck list counts and a finished deck's facts

Spec: sched/counts
Requirements: R34, R39, R78, R80, R93, R434, R624, R625, R626, R628, R767, R1819, R1897
Anki: 26.09.3
Scenarios: conformance/scenarios/sched/counts/

The New, Learn and Due numbers of every deck in the deck list, the numbers of a deck's own page, and the facts a deck with nothing left to study reports (`DeckStudyState`). The tree's shape is `decks/decks` (U28); remaining limits are `sched/limits#L2`; the queue is `sched/queue-building`.

## C1. The tree

- The deck list shows one node per deck, filtered decks included, nested by name, siblings in `unicase` order of their full stored names (`text/unicode-primitives#U2`). So "10. y" sorts before "4. x" and "9. z", "a" before "B", and letters with accents and emoji after ASCII (R34; golden `sched/counts/tree-order`).
- A deck whose parent is missing is left out with its descendants (`decks/decks`, the deck tree; Anki attaches it to the node before it, a difference U28 records). It is also left out of every queue (`sched/limits#L3`).

## C2. The Default deck (R39)

The Default deck (id 1) is left out of the tree when all of these hold: it is top-level, it has no child deck, no card has `did` 1, and some other top-level deck exists. Cards of the Default deck that sit in a filtered deck do not count. Goldens: `sched/counts/tree-order` (hidden), `sched/counts/default-only` (the only deck: shown), `sched/counts/default-shown` (shown with a child deck; shown with a card).

## C3. The counts of one deck (R624, R626)

**Own counts.** For each deck, from the cards whose `did` is that deck, at instant `now` on day `today`, with L the learn-ahead limit (`collapseTime`):

| Count | Cards |
|---|---|
| new | queue 0 |
| review | queue 2, `due` ≤ today |
| interday learning | queue 3, `due` ≤ today |
| intraday learning | queue 1 with `due` < now + L, and queue 4 with `due` ≤ now + L |

The intraday test is strict for queue 1: a learning card due exactly L seconds from now is in the queue (`sched/queue-building#B6`) but not yet in the deck list (golden `sched/queue/learn-ahead/ready-in`). Queue 4 counts up to and including now + L (golden `sched/queue/gather/preview-in-normal-deck`, a queue 4 card left in a normal deck, `sched/queue-building#B4`).

**Sums and limits.** Working up from the leaves, each node's totals are its own counts plus its children's limited totals. Then the node's limits are applied to its totals, using the node's remaining limits `R` (C4):

1. interday learning becomes the smaller of it and R.review;
2. reviews left = R.review − that interday learning;
3. review becomes the smaller of it and reviews left;
4. new becomes the smaller of it and R.new;
5. when the node's deck caps new cards by reviews (`sched/limits#L2`): new becomes the smaller of it and (reviews left − review);
6. intraday learning is never limited.

The node shows **New** = new, **Learn** = intraday learning + interday learning, **Due** = review. Learn counts cards, not steps.

Example: one subtree has a preset allowing 5 new and 8 review cards. A branch permits 3 reviews today; its leaf permits 1 new card today. Another branch uses the ordinary preset limits.

| Node | Direct cards | Effective limits | Display N, L, D |
|---|---|---|---|
| root::one::leaf | 3 new, 2 review | 1, 200 | 1, 0, 2 |
| root::one | 2 review | 3, 3 | 0, 0, 3 |
| root::two | 4 new, 3 review | 20, 200 | 4, 0, 3 |
| root | 1 review | 5, 8 | 1, 0, 7 |

The saved `sched/counts/limits-tree` case checks this limit interaction with its own deck names, and checks a separate top-level deck too.

## C4. The limits of a node

A normal node uses L2's limits for today. Filtered nodes and nodes with no stored preset use 9999 for each limit; reviews do not cap their new cards.

With `applyAllParentLimits` off, each node applies its own limits to the total formed in C3. Its parent can further reduce that parent's displayed total, without changing the child's display.

With the option on, limits also pass down the tree. Each node first uses the lesser of its own and its parent's effective limit, separately for new and review cards. At the top level the parent limits are 9999. This can reduce a child's counts compared with the off case; the saved limit-tree case checks both settings.

No count subtracts siblings skipped during a queue build (R625). Every matching card contributes before limits. The count calculation reads the stored rows and limits directly; it never consumes a queue.

## C5. The deck page (R625, R767)

- A deck's page shows the counts of its queue (`sched/queue-building#B9`), built as the study screen would build it.
- For each of New, Learn and Due it also shows the deck list's count minus the queue's count, when that difference is not 0, as a signed number. It is positive when the queue skips siblings or limits a subdeck more strictly, and can be negative (Learn, for a learning card due exactly L from now, C3).
- Golden `sched/counts/deck-page-difference`: deck list 4 new, 0 learning, 6 reviews; queue 2, 1 and 3; differences +2, −1 and +3.

## C6. When counts change, and caching them (R1819)

- Within a day and with no change to the collection, a deck's counts change only when an intraday learning card starts to count: a queue 1 card at `due` − L + 1, a queue 4 card at `due` − L. The deck list record carries **next_change_at**: the earliest of those instants still after now, and `next_day_at`. Home arms one timer on it.
- Counts may be cached in the derived store (U26) under the key (day number, `col.mod`), valid until `next_change_at`. A cached value whose day is not the current day, whose `col.mod` differs, or whose `next_change_at` has passed is never served; the counts are computed again.
- Production reads complete automatic maintenance (`sched/day-clock#D8`) before opening the read-only count snapshot. The count calculation writes nothing. Automatic maintenance restores eligible buried cards, preserves missing rollover values and writes no empty-unbury marker. It runs outside a busy writer operation. Conformance `deck_counts` separately retains Q3's explicit compatibility prelude; it must not become the production read helper.
- A generation replacement or whole-collection invalidation discards the cached record, even when the numeric day and `col.mod` match.

## C7. A finished deck's facts (`DeckStudyState`)

One record per deck at one instant. Queue fields use the normal studied subtree (`sched/queue-building#B2`); compatibility facts use the full named subtree, filtered descendants included, by `did`. This record feeds the deck page, finished state and session summary (R78, R80, R93, R434, R628). It keeps two scopes explicit:

| Member | Value |
|---|---|
| `queue_counts` | B9's counts of the real queue |
| `list_counts`, `difference` | C3's deck-list counts and C5's signed per-kind differences |
| `finished` | no presented learning card within now + L and no main-list card |
| `learning_remaining_today` | queue 1 or normal-deck queue 4 cards with `due` ≤ `next_day_at`, including overdue cards |
| `learning_later_today` | those cards with now < `due` ≤ `next_day_at` |
| `next_learning_due` | minimum due among `learning_later_today`; absent when that set is empty |
| `next_learning_ready_at` | `next_learning_due` − L; absent with it; it may already be at or before now |
| `review_limit_reached`, `new_limit_reached` | a review/interday or new card otherwise eligible for this queue remains excluded by its applicable daily limits, including ancestor/review caps; false when limits exclude no such card |
| `learning_remaining` | compatibility total of every queue-1 card in the full subtree, regardless of due instant |
| `secs_until_next_learn` | compatibility seconds: 86,400 with no queue-1 card; otherwise max(60, minimum queue-1 due − L − now), over that full subtree |
| `review_remaining` | compatibility flag: some queue-2 or queue-3 card in the full subtree has `due` ≤ today |
| `new_remaining` | compatibility flag: some card in that full subtree is in queue 0 |
| `sibling_buried`, `user_buried` | exact counts of queues −2 and −3 in the full subtree; Unbury offers each kind whose count exceeds 0 |
| `tomorrow` | C8's projected deck-list record |
| `empty` | C9's membership test |

The native today window includes the exact `next_day_at` boundary, as B4's gather rule does. It excludes imported queue-1 timestamps beyond that boundary. Such a row still contributes to compatibility `learning_remaining` and `secs_until_next_learn`. A tomorrow-only row must not supply a native today ready instant. These native fields are Meguri presentation requirements, not additional claims about Anki's congratulations fields.

The six `congrats` output fields retain their recorded scope: `learning_remaining`, `secs_until_next_learn`, the two compatibility remaining flags, and whether each buried count exceeds 0 (`conformance/queue-ops#Q4`). They do not substitute for the native limit flags or today-only counts. Existing goldens establish the ordinary cases below; boundary/imported-state compatibility outputs still need focused oracle evidence before extra parity claims.

- Golden `sched/counts/finished`: limits used up, 3 reviews due, 2 new, one card of each buried kind, learning due in 30 minutes and 2 hours. The queue is empty; compatibility total 2, seconds 600, both remaining and buried flags true. An empty deck gives 0, 86,400 and false flags.
- Golden `sched/queue/learn-ahead/ready-in`: due in 30 minutes with L 1,200 gives a native ready instant 600 seconds away. Compatibility seconds clamp to 60 even one second before readiness. At readiness the queue presents the card.

Own tests must separately cover overdue, due-now, within-learn-ahead, later-today, exact-boundary and beyond-boundary rows, with queues 1 and 4. Check absence of native next instants, both limit flags, exact buried counts, C8's projection and C9's home-deck membership.

## C8. Tomorrow's counts

`tomorrow` holds the New, Learn and Due counts the deck list will show for the deck at `next_day_at`, if nothing else changes before then: C3 with today + 1, now = `next_day_at`, the studied counters read as 0, the limits in force on today + 1 (a Today-only limit set for today lapses), and every card in queue −2 or −3 counted in the queue the start-of-day unbury gives it (`sched/day-clock#D7`). Anki has no such number; the summary shows it (R93). Golden `sched/counts/finished`, last read: the deck list at 04:00 the next day shows what `tomorrow` held for Done at 10:00.

## C9. An empty deck

`empty` is true when no card has its `did` or its `odid` in the subtree. A deck whose cards all sit in a filtered deck elsewhere is not empty: its page shows the finished state, not "This deck is empty" (R80). Anki has no such flag.

## C10. Scenarios

| Scenario | Covers |
|---|---|
| `sched/counts/tree-order` | C1, C2 (hidden Default) |
| `sched/counts/default-only`, `default-shown` | C2 |
| `sched/counts/limits-tree` | C3, C4 at 10:00, 03:59:59 and 04:00:01 (R1897), and with `applyAllParentLimits` on |
| `sched/counts/ae84-kathmandu` | C3 around a 04:00 rollover in Asia/Kathmandu (AE84) |
| `sched/counts/deck-page-difference` | C4 (no sibling subtraction), C5 |
| `sched/counts/finished` | C7, C8 |
| `sched/queue/learn-ahead/window`, `ready-in` | C3's strict test, C7 |
| `sched/queue/gather/card-states`, `nested-filtered` | C1 (filtered decks counted under their parent), C7 |
| `sched/queue/gather/preview-in-normal-deck` | C3 (queue 4's test), C7 (`learning_remaining` counts queue 1 only) |

Meguri's required tests must check C6 (next_change_at, the cache key, never serving another day's counts), C8 against the goldens' next-day reads, and C9, which the oracle cannot observe.
